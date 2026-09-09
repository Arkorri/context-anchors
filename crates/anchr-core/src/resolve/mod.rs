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
            match root.config.ignore.path_pattern(path.as_path(), is_dir) {
                Some(pattern) => format!(
                    "`{path}` exists on disk, but `[ignore] paths` pattern `{pattern}` keeps it out of the scan; ignored paths cannot be referenced"
                ),
                None if is_dir => format!(
                    "`{path}` exists on disk, but the scan found no files beneath it (empty, or everything in it is ignored); such directories cannot be referenced"
                ),
                None => format!(
                    "`{path}` exists on disk, but is ignored by `.gitignore`, so the scan never sees it; ignored files cannot be referenced"
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
mod tests;
