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
            // A `.git` marker: `.gitignore` files carry weight only inside a repository.
            fs::create_dir_all(base.join(name).join(".git")).unwrap();
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
            let output = scan_root(root, &registry, mode);
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
    config.ignore.paths = ignore::gitignore::GitignoreBuilder::new("/repo")
        .add_line(None, "vendor/**")
        .unwrap()
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
        excluded.contains("`[ignore] paths` pattern `vendor/**`"),
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
