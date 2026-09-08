//! Does a reference target exist? Three answers: yes, no, and "could not check", and the
//! third is never allowed to look like the first.

mod path;
mod symbol;

use std::collections::BTreeMap;

use camino::Utf8PathBuf;

use crate::index::Index;
use crate::marker::{AnchorId, PathExpectation, RefTarget, RelPath, SymbolName};
use crate::root::{FilePath, Root, RootName, RootSet, RootStatus};
use crate::text::{FileAnalyzer, LanguageRegistry};

pub use path::EntryKind;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Resolution {
    Resolved,
    Unresolved(Unresolved),
    Unverified(Unverified),
}

/// The target does not exist. Always an error.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Unresolved {
    PathMissing {
        root: RootName,
        path: RelPath,
    },
    /// The target ended in `/` but names a file.
    PathNotDirectory {
        root: RootName,
        path: RelPath,
    },
    /// A symbol target names a directory.
    PathNotFile {
        root: RootName,
        path: RelPath,
    },
    /// The path exists but, through a symlink, lives outside the root; it is never read.
    PathEscapesRoot {
        root: RootName,
        path: RelPath,
    },
    SymbolMissing {
        root: RootName,
        path: RelPath,
        name: SymbolName,
    },
    AnchorMissing {
        root: RootName,
        id: AnchorId,
    },
    /// The `root:` prefix names no declared root; a typo, not an absent root.
    RootUndeclared {
        name: RootName,
    },
}

/// The target could not be checked. Reported, never silently passed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Unverified {
    RootAbsent {
        name: RootName,
        declared_dir: Utf8PathBuf,
    },
    NoGrammar {
        root: RootName,
        path: RelPath,
        extension: Option<String>,
    },
    /// The symbol was not found, but the parse tree has errors, so it may be hidden in one.
    ParseErrors {
        root: RootName,
        path: RelPath,
        language: &'static str,
    },
    ParseTimeout {
        root: RootName,
        path: RelPath,
    },
    SymbolTableTruncated {
        root: RootName,
        path: RelPath,
    },
    TargetTooLarge {
        root: RootName,
        path: RelPath,
        bytes: u64,
        limit: u64,
    },
    TargetNotUtf8 {
        root: RootName,
        path: RelPath,
    },
    TargetUnreadable {
        root: RootName,
        path: RelPath,
        message: String,
    },
    AnalyzeFailed {
        root: RootName,
        path: RelPath,
        message: String,
    },
}

/// A root together with its index, or the record that it is declared but absent.
#[derive(Debug)]
pub enum IndexedRoot {
    Present { root: Box<Root>, index: Index },
    Absent { declared_dir: Utf8PathBuf },
}

/// Every declared root, indexed where present. Built once per check; the resolver's input.
#[derive(Debug)]
pub struct IndexedRoots {
    current: RootName,
    roots: BTreeMap<RootName, IndexedRoot>,
}

impl IndexedRoots {
    /// `indexes` must hold one entry per present root in `roots`.
    pub fn new(roots: &RootSet, mut indexes: BTreeMap<RootName, Index>) -> Self {
        let mut entries = BTreeMap::new();
        for name in roots.names() {
            let entry = match roots.status(name) {
                Some(RootStatus::Present(root)) => match indexes.remove(name) {
                    Some(index) => IndexedRoot::Present {
                        root: root.clone(),
                        index,
                    },
                    None => IndexedRoot::Present {
                        root: root.clone(),
                        index: Index::new(name.clone()),
                    },
                },
                Some(RootStatus::Absent { declared_dir, .. }) => IndexedRoot::Absent {
                    declared_dir: declared_dir.clone(),
                },
                None => continue,
            };
            entries.insert(name.clone(), entry);
        }
        Self {
            current: roots.current_name().clone(),
            roots: entries,
        }
    }

    pub fn current_name(&self) -> &RootName {
        &self.current
    }

    pub fn get(&self, name: &RootName) -> Option<&IndexedRoot> {
        self.roots.get(name)
    }

    pub fn current(&self) -> (&Root, &Index) {
        match self.roots.get(&self.current) {
            Some(IndexedRoot::Present { root, index }) => (root.as_ref(), index),
            // `new` always records the current root as present.
            _ => unreachable!("current root is always present"),
        }
    }

    pub fn current_mut(&mut self) -> (&Root, &mut Index) {
        match self.roots.get_mut(&self.current) {
            Some(IndexedRoot::Present { root, index }) => (&**root, index),
            _ => unreachable!("current root is always present"),
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &RootName> {
        self.roots.keys()
    }

    pub fn present(&self) -> impl Iterator<Item = (&Root, &Index)> {
        self.roots.values().filter_map(|entry| match entry {
            IndexedRoot::Present { root, index } => Some((root.as_ref(), index)),
            IndexedRoot::Absent { .. } => None,
        })
    }

    pub fn external(&self) -> impl Iterator<Item = (&RootName, &IndexedRoot)> {
        self.roots.iter().filter(|(name, _)| **name != self.current)
    }
}

/// Resolves targets against a set of indexed roots. Single-threaded; owns its caches.
pub struct Resolver<'a> {
    roots: &'a IndexedRoots,
    analyzer: FileAnalyzer<'a>,
    paths: path::PathResolver,
    symbols: symbol::SymbolCache,
}

impl<'a> Resolver<'a> {
    pub fn new(roots: &'a IndexedRoots, registry: &'a LanguageRegistry) -> Self {
        let parse_budget = roots.current().0.config.scan.parse_budget;
        Self {
            roots,
            analyzer: FileAnalyzer::new(registry, parse_budget),
            paths: path::PathResolver::default(),
            symbols: symbol::SymbolCache::default(),
        }
    }

    /// `written_in` is the root the reference appears in; a bare target resolves there.
    pub fn resolve(&mut self, written_in: &RootName, target: &RefTarget) -> Resolution {
        let root_name = target.root().unwrap_or(written_in);
        let (root, index) = match self.select_root(root_name) {
            Ok(selected) => selected,
            Err(resolution) => return resolution,
        };
        match target {
            RefTarget::Path { path, expects, .. } => self.resolve_path(root, index, path, *expects),
            RefTarget::Symbol { path, name, .. } => self.resolve_symbol(root, index, path, name),
            RefTarget::Anchor { id, .. } => resolve_anchor(index, id),
        }
    }

    /// One suggestion for an unresolved target, or none. Never a correctness claim.
    pub fn suggest(&mut self, unresolved: &Unresolved) -> Option<String> {
        match unresolved {
            Unresolved::AnchorMissing { root, id } => {
                let (_, index) = self.present(root)?;
                crate::suggest::suggest_anchor(id, index.anchor_ids())
            }
            Unresolved::SymbolMissing { root, path, name } => {
                let table = self.symbols.cached(root, path)?;
                crate::suggest::suggest(name.as_str(), table.names())
            }
            Unresolved::PathMissing { root, path } => {
                let (root, index) = self.present(root)?;
                self.paths.suggest(root, index.tree(), path)
            }
            Unresolved::RootUndeclared { name } => {
                crate::suggest::suggest(name.as_str(), self.roots.names().map(RootName::as_str))
            }
            Unresolved::PathNotDirectory { .. }
            | Unresolved::PathNotFile { .. }
            | Unresolved::PathEscapesRoot { .. } => None,
        }
    }

    /// Why a missing path may nevertheless be on this machine's disk: context for the reader,
    /// never a fix. The scan decides existence; the disk only explains the disagreement.
    pub fn explain(&self, unresolved: &Unresolved) -> Option<String> {
        let Unresolved::PathMissing { root, path } = unresolved else {
            return None;
        };
        let (root, _) = self.present(root)?;
        let on_disk = std::fs::symlink_metadata(root.dir.join(path.as_path())).ok()?;
        let is_dir = on_disk.is_dir();
        Some(
            match root.config.scan.exclude_matching(path.as_path(), is_dir) {
                Some(pattern) => format!(
                    "`{path}` exists on disk, but `[scan] exclude` pattern `{pattern}` keeps it out of the scan; excluded paths cannot be referenced"
                ),
                None if is_dir => format!(
                    "`{path}` exists on disk, but the scan found no files beneath it (empty, or everything in it is ignored); such directories cannot be referenced"
                ),
                None => format!(
                    "`{path}` exists on disk, but is ignored by `.gitignore` or `.anchrignore` rules, so the scan never sees it; ignored files cannot be referenced"
                ),
            },
        )
    }

    /// When a missing bare path exists beside the file that wrote it, says so. Per site, never
    /// per cause: the same name may be right next to one file and nowhere near another.
    pub fn relative_note(&self, unresolved: &Unresolved, written_in: &FilePath) -> Option<String> {
        let Unresolved::PathMissing { root, path } = unresolved else {
            return None;
        };
        if root != self.roots.current_name() {
            return None;
        }
        let (root, index) = self.present(root)?;
        let dir = written_in.directory();
        if dir.as_str().is_empty() {
            return None;
        }
        let anchored = RelPath::parse(&format!("{dir}/{path}")).ok()?;
        matches!(
            self.paths.locate(root, index.tree(), &anchored),
            path::Located::Found { .. }
        )
        .then(|| format!("in `{written_in}`, `./{path}` would resolve to `{anchored}`"))
    }

    fn select_root(&self, name: &RootName) -> Result<(&'a Root, &'a Index), Resolution> {
        match self.roots.get(name) {
            Some(IndexedRoot::Present { root, index }) => Ok((root.as_ref(), index)),
            Some(IndexedRoot::Absent { declared_dir }) => {
                Err(Resolution::Unverified(Unverified::RootAbsent {
                    name: name.clone(),
                    declared_dir: declared_dir.clone(),
                }))
            }
            None => Err(Resolution::Unresolved(Unresolved::RootUndeclared {
                name: name.clone(),
            })),
        }
    }

    fn present(&self, name: &RootName) -> Option<(&'a Root, &'a Index)> {
        match self.roots.get(name) {
            Some(IndexedRoot::Present { root, index }) => Some((root.as_ref(), index)),
            _ => None,
        }
    }

    fn resolve_path(
        &mut self,
        root: &Root,
        index: &Index,
        path: &RelPath,
        expects: PathExpectation,
    ) -> Resolution {
        match self.paths.locate(root, index.tree(), path) {
            path::Located::Missing { .. } => Resolution::Unresolved(Unresolved::PathMissing {
                root: root.name.clone(),
                path: path.clone(),
            }),
            path::Located::Found { kind, .. } => match (expects, kind) {
                (PathExpectation::Directory, EntryKind::File) => {
                    Resolution::Unresolved(Unresolved::PathNotDirectory {
                        root: root.name.clone(),
                        path: path.clone(),
                    })
                }
                (PathExpectation::Any, _) | (PathExpectation::Directory, EntryKind::Directory) => {
                    Resolution::Resolved
                }
            },
        }
    }

    fn resolve_symbol(
        &mut self,
        root: &Root,
        index: &Index,
        path: &RelPath,
        name: &SymbolName,
    ) -> Resolution {
        let absolute = match self.paths.locate(root, index.tree(), path) {
            path::Located::Missing { .. } => {
                return Resolution::Unresolved(Unresolved::PathMissing {
                    root: root.name.clone(),
                    path: path.clone(),
                });
            }
            path::Located::Found {
                kind: EntryKind::Directory,
                ..
            } => {
                return Resolution::Unresolved(Unresolved::PathNotFile {
                    root: root.name.clone(),
                    path: path.clone(),
                });
            }
            path::Located::Found { absolute, .. } => absolute,
        };
        if !self.paths.stays_within(root, &absolute) {
            return Resolution::Unresolved(Unresolved::PathEscapesRoot {
                root: root.name.clone(),
                path: path.clone(),
            });
        }
        let table = match self.symbols.load(root, path, &absolute, &mut self.analyzer) {
            Ok(table) => table,
            Err(unverified) => return Resolution::Unverified(unverified),
        };
        if table.contains(name) {
            Resolution::Resolved
        } else if table.has_parse_errors {
            Resolution::Unverified(Unverified::ParseErrors {
                root: root.name.clone(),
                path: path.clone(),
                language: table.language,
            })
        } else {
            Resolution::Unresolved(Unresolved::SymbolMissing {
                root: root.name.clone(),
                path: path.clone(),
                name: name.clone(),
            })
        }
    }
}

fn resolve_anchor(index: &Index, id: &AnchorId) -> Resolution {
    if index.anchor_sites(id).is_empty() {
        Resolution::Unresolved(Unresolved::AnchorMissing {
            root: index.root().clone(),
            id: id.clone(),
        })
    } else {
        Resolution::Resolved
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use camino::Utf8Path;

    use super::*;
    use crate::config::Config;
    use crate::marker::parse_target;
    use crate::scan::{ScanMode, scan_root};

    struct World {
        _dir: tempfile::TempDir,
        roots: IndexedRoots,
        registry: LanguageRegistry,
    }

    impl World {
        /// `roots` maps root name → files. The first entry is the current root.
        fn new(roots: &[(&str, &[(&str, &str)])]) -> Self {
            Self::build(roots, Config::default(), |_| {})
        }

        /// `setup` runs against the current root's directory after the files are written and
        /// before the scan, for symlinks and empty directories the scan must see.
        fn build(
            roots: &[(&str, &[(&str, &str)])],
            mut current_config: Config,
            setup: impl FnOnce(&Utf8Path),
        ) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
            let registry = LanguageRegistry::new().unwrap();

            for (name, _) in roots.iter().skip(1) {
                current_config
                    .external_roots
                    .insert(RootName::parse(name).unwrap(), base.join(name));
            }
            current_config
                .external_roots
                .insert(RootName::parse("absent").unwrap(), base.join("not-created"));
            for (name, files) in roots {
                for (path, contents) in *files {
                    let full = base.join(name).join(path);
                    fs::create_dir_all(full.parent().unwrap()).unwrap();
                    fs::write(full, contents).unwrap();
                }
            }
            let (current_name, _) = roots[0];
            setup(&base.join(current_name));
            let root_set = RootSet::load(base.join(current_name), current_config).unwrap();

            let mut indexes = BTreeMap::new();
            for root in root_set.present() {
                let mode = if root.name == *root_set.current_name() {
                    ScanMode::Full
                } else {
                    ScanMode::AnchorsOnly
                };
                let output = scan_root(root, &registry, mode).unwrap();
                indexes.insert(
                    root.name.clone(),
                    Index::from_scan(root.name.clone(), output.files, output.tree),
                );
            }
            Self {
                _dir: dir,
                roots: IndexedRoots::new(&root_set, indexes),
                registry,
            }
        }

        fn resolve(&self, target: &str) -> Resolution {
            let mut resolver = Resolver::new(&self.roots, &self.registry);
            let target = parse_target(target, None).unwrap().target;
            resolver.resolve(self.roots.current_name(), &target)
        }

        fn unresolved(&self, target: &str) -> Unresolved {
            match self.resolve(target) {
                Resolution::Unresolved(unresolved) => unresolved,
                other => panic!("expected unresolved, got {other:?}"),
            }
        }

        /// Resolves and suggests with one resolver: symbol suggestions read the cache the
        /// resolution filled.
        fn suggestion(&self, target: &str) -> Option<String> {
            let mut resolver = Resolver::new(&self.roots, &self.registry);
            let target = parse_target(target, None).unwrap().target;
            match resolver.resolve(self.roots.current_name(), &target) {
                Resolution::Unresolved(unresolved) => resolver.suggest(&unresolved),
                other => panic!("expected unresolved, got {other:?}"),
            }
        }

        fn explanation(&self, target: &str) -> Option<String> {
            let resolver = Resolver::new(&self.roots, &self.registry);
            resolver.explain(&self.unresolved(target))
        }
    }

    fn is_missing(resolution: &Resolution) -> bool {
        matches!(
            resolution,
            Resolution::Unresolved(Unresolved::PathMissing { .. })
        )
    }

    fn world() -> World {
        World::new(&[
            (
                "repo",
                &[
                    (
                        "docs/guide.md",
                        "# Guide @anchor[guide] @anchor[auth/token-refresh]",
                    ),
                    (
                        "src/auth.rs",
                        "pub fn validate_token() {}\npub struct Session;",
                    ),
                    ("src/broken.rs", "fn intact() {}\nfn broken( {"),
                    ("src/weird.ex", "defmodule X do end"),
                    ("src/Mixed.ts", "export const mixedCase = 1;"),
                ],
            ),
            (
                "plugin",
                &[("SKILL.md", "@anchor[skill/entry] @anchor[skill/entry]")],
            ),
        ])
    }

    #[test]
    fn paths_resolve_exactly_and_directories_are_distinguished() {
        let w = world();
        assert_eq!(w.resolve("docs/guide.md"), Resolution::Resolved);
        assert_eq!(w.resolve("docs"), Resolution::Resolved);
        assert_eq!(w.resolve("docs/"), Resolution::Resolved);
        assert!(matches!(
            w.resolve("docs/guide.md/"),
            Resolution::Unresolved(Unresolved::PathNotDirectory { .. })
        ));
        assert!(matches!(
            w.resolve("docs/missing.md"),
            Resolution::Unresolved(Unresolved::PathMissing { .. })
        ));
        // Exact-name lookup: a case mismatch is missing on every platform.
        assert!(matches!(
            w.resolve("src/mixed.ts"),
            Resolution::Unresolved(Unresolved::PathMissing { .. })
        ));
        assert!(matches!(
            w.resolve("Docs/guide.md"),
            Resolution::Unresolved(Unresolved::PathMissing { .. })
        ));
    }

    #[test]
    fn symbols_resolve_against_declarations() {
        let w = world();
        assert_eq!(
            w.resolve("src/auth.rs#validate_token"),
            Resolution::Resolved
        );
        assert_eq!(w.resolve("src/auth.rs#Session"), Resolution::Resolved);
        assert_eq!(w.resolve("src/Mixed.ts#mixedCase"), Resolution::Resolved);
        assert!(matches!(
            w.resolve("src/auth.rs#refresh_token"),
            Resolution::Unresolved(Unresolved::SymbolMissing { .. })
        ));
        assert!(matches!(
            w.resolve("src/missing.rs#f"),
            Resolution::Unresolved(Unresolved::PathMissing { .. })
        ));
        assert!(matches!(
            w.resolve("src#f"),
            Resolution::Unresolved(Unresolved::PathNotFile { .. })
        ));
    }

    #[test]
    fn unverifiable_symbols_are_never_errors() {
        let w = world();
        assert!(matches!(
            w.resolve("src/weird.ex#anything"),
            Resolution::Unverified(Unverified::NoGrammar { extension: Some(ext), .. }) if ext == "ex"
        ));
        assert_eq!(w.resolve("src/broken.rs#intact"), Resolution::Resolved);
        assert!(matches!(
            w.resolve("src/broken.rs#hidden"),
            Resolution::Unverified(Unverified::ParseErrors {
                language: "rust",
                ..
            })
        ));
    }

    #[test]
    fn anchors_resolve_locally_and_across_roots() {
        let w = world();
        assert_eq!(w.resolve("#guide"), Resolution::Resolved);
        assert_eq!(w.resolve("#auth/token-refresh"), Resolution::Resolved);
        assert_eq!(w.resolve("plugin:#skill/entry"), Resolution::Resolved);
        assert!(matches!(
            w.resolve("#skill/entry"),
            Resolution::Unresolved(Unresolved::AnchorMissing { .. })
        ));
        assert!(matches!(
            w.resolve("plugin:#guide"),
            Resolution::Unresolved(Unresolved::AnchorMissing { .. })
        ));
    }

    #[test]
    fn root_selection_separates_typos_from_absence_for_every_kind() {
        let w = world();
        for target in ["absent:#x", "absent:docs/guide.md", "absent:a.rs#f"] {
            assert!(matches!(
                w.resolve(target),
                Resolution::Unverified(Unverified::RootAbsent { .. })
            ));
        }
        for target in ["nope:#x", "nope:docs/guide.md", "nope:a.rs#f"] {
            assert!(matches!(
                w.resolve(target),
                Resolution::Unresolved(Unresolved::RootUndeclared { .. })
            ));
        }
    }

    #[test]
    fn suggestions_come_from_the_right_candidate_set() {
        let w = world();
        assert_eq!(w.suggestion("#guid").as_deref(), Some("guide"));
        assert_eq!(
            w.suggestion("#token-refresh").as_deref(),
            Some("auth/token-refresh")
        );
        assert_eq!(
            w.suggestion("src/auth.rs#validateToken").as_deref(),
            Some("validate_token")
        );
        assert_eq!(
            w.suggestion("docs/guid.md").as_deref(),
            Some("docs/guide.md")
        );
        assert_eq!(
            w.suggestion("src/mixed.ts").as_deref(),
            Some("src/Mixed.ts")
        );
        assert_eq!(w.suggestion("plugn:#x").as_deref(), Some("plugin"));
        assert_eq!(w.suggestion("#completely-different"), None);
    }

    #[test]
    fn a_missing_bare_path_beside_its_file_gets_a_relative_note() {
        let w = world();
        let resolver = Resolver::new(&w.roots, &w.registry);
        let missing = w.unresolved("guide.md");
        let in_docs = FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap();
        let in_src = FilePath::new(Utf8PathBuf::from("src/a.md")).unwrap();
        let at_root = FilePath::new(Utf8PathBuf::from("a.md")).unwrap();
        assert_eq!(
            resolver.relative_note(&missing, &in_docs).as_deref(),
            Some("in `docs/a.md`, `./guide.md` would resolve to `docs/guide.md`")
        );
        assert_eq!(resolver.relative_note(&missing, &in_src), None);
        assert_eq!(resolver.relative_note(&missing, &at_root), None);
        assert_eq!(
            resolver.relative_note(&w.unresolved("#nope"), &in_docs),
            None
        );
    }

    #[test]
    fn existence_follows_the_scan_not_the_disk() {
        let mut config = Config::default();
        config.scan.exclude_patterns = vec!["vendor/**".to_owned()];
        config.scan.exclude = globset::GlobSetBuilder::new()
            .add(
                globset::GlobBuilder::new("vendor/**")
                    .literal_separator(true)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let w = World::build(
            &[(
                "repo",
                &[
                    (".gitignore", "build/\nserver/lib/\n"),
                    ("build/out.md", ""),
                    ("vendor/lib.md", ""),
                    ("Cargo.lock", ""),
                    ("server/lib/x.md", ""),
                    ("server/app.md", ""),
                    ("docs/guide.md", ""),
                ],
            )],
            config,
            |root| fs::create_dir_all(root.join("empty")).unwrap(),
        );
        assert_eq!(w.resolve("Cargo.lock"), Resolution::Resolved);
        assert_eq!(w.resolve("server/"), Resolution::Resolved);
        assert_eq!(w.resolve("docs/"), Resolution::Resolved);
        for target in [
            "build/out.md",
            "build/",
            "vendor/lib.md",
            "vendor/",
            "empty/",
            "server/lib",
            "server/lib/",
            "server/lib/x.md",
        ] {
            assert!(is_missing(&w.resolve(target)), "{target}");
        }
        assert_eq!(w.suggestion("build/out.md"), None);

        let excluded = w.explanation("vendor/lib.md").unwrap();
        assert!(
            excluded.contains("`[scan] exclude` pattern `vendor/**`"),
            "{excluded}"
        );
        let ignored = w.explanation("build/out.md").unwrap();
        assert!(ignored.contains("ignored by `.gitignore`"), "{ignored}");
        let empty = w.explanation("empty/").unwrap();
        assert!(empty.contains("no files beneath it"), "{empty}");
        assert_eq!(w.explanation("docs/nope.md"), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_redirect_inside_the_root_and_fall_back_to_the_disk_outside_it() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn leaked() {}").unwrap();
        let outside_dir = outside.path().to_path_buf();
        let w = World::build(
            &[(
                "repo",
                &[
                    ("docs/guide.md", "# Guide"),
                    ("src/auth.rs", "pub fn validate_token() {}"),
                ],
            )],
            Config::default(),
            move |root| {
                use std::os::unix::fs::symlink;
                symlink(outside_dir.join("secret.rs"), root.join("src/link.rs")).unwrap();
                symlink("/nonexistent", root.join("src/dangling.rs")).unwrap();
                symlink("auth.rs", root.join("src/alias.rs")).unwrap();
                symlink("docs", root.join("docs-link")).unwrap();
            },
        );
        assert!(matches!(
            w.resolve("src/link.rs#leaked"),
            Resolution::Unresolved(Unresolved::PathEscapesRoot { .. })
        ));
        assert_eq!(w.resolve("src/link.rs"), Resolution::Resolved);
        assert!(is_missing(&w.resolve("src/dangling.rs")));
        assert_eq!(
            w.resolve("src/alias.rs#validate_token"),
            Resolution::Resolved
        );
        assert_eq!(w.resolve("docs-link/guide.md"), Resolution::Resolved);
        assert_eq!(w.resolve("docs-link/"), Resolution::Resolved);
        assert!(is_missing(&w.resolve("docs-link/nope.md")));
    }
}
