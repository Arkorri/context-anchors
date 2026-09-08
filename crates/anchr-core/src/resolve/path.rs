use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::marker::RelPath;
use crate::root::{Root, RootName};
use crate::tree::{FileTree, Lookup};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

pub(super) enum Located {
    Found {
        kind: EntryKind,
        absolute: Utf8PathBuf,
    },
    Missing {
        /// The deepest existing directory (root-relative, `""` for the root) and the name
        /// that was not there.
        parent: String,
        name: String,
    },
}

/// Existence is the scan's tree, not the disk: a local run and a clean CI checkout agree, and
/// exact-byte keys keep a reference from resolving against a differently cased file on macOS.
/// The one disk touch is a symlink that leaves the root, which carries no ignore information.
#[derive(Default)]
pub(super) struct PathResolver {
    canonical_roots: HashMap<RootName, Option<Utf8PathBuf>>,
}

impl PathResolver {
    pub(super) fn locate(&self, root: &Root, tree: &FileTree, path: &RelPath) -> Located {
        let absolute = root.dir.join(path.as_path());
        match tree.lookup(path.as_path()) {
            Lookup::File => Located::Found {
                kind: EntryKind::File,
                absolute,
            },
            Lookup::Directory => Located::Found {
                kind: EntryKind::Directory,
                absolute,
            },
            Lookup::Missing { parent, name } => Located::Missing { parent, name },
            Lookup::LeavesRoot => match fs::metadata(&absolute) {
                Ok(target) if target.is_dir() => Located::Found {
                    kind: EntryKind::Directory,
                    absolute,
                },
                Ok(target) if target.is_file() => Located::Found {
                    kind: EntryKind::File,
                    absolute,
                },
                _ => Located::Missing {
                    parent: path.parent().map(|p| p.to_string()).unwrap_or_default(),
                    name: path.file_name().to_owned(),
                },
            },
        }
    }

    /// Canonical containment check for files that are about to be read.
    pub(super) fn stays_within(&mut self, root: &Root, absolute: &Utf8Path) -> bool {
        let canonical_root = self
            .canonical_roots
            .entry(root.name.clone())
            .or_insert_with(|| root.dir.canonicalize_utf8().ok().map(simplified));
        let Some(canonical_root) = canonical_root else {
            return false;
        };
        match absolute.canonicalize_utf8() {
            Ok(canonical) => simplified(canonical).starts_with(canonical_root),
            Err(_) => false,
        }
    }

    /// A sibling of the missing component, if one is close, spliced into the path.
    pub(super) fn suggest(&self, root: &Root, tree: &FileTree, path: &RelPath) -> Option<String> {
        let Located::Missing { parent, name } = self.locate(root, tree, path) else {
            return None;
        };
        let candidate = crate::suggest::suggest(&name, tree.children(&parent))?;
        Some(if parent.is_empty() {
            candidate
        } else {
            format!("{parent}/{candidate}")
        })
    }
}

#[cfg(windows)]
fn simplified(path: Utf8PathBuf) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dunce::simplified(path.as_std_path()).to_path_buf()).unwrap_or(path)
}

#[cfg(not(windows))]
fn simplified(path: Utf8PathBuf) -> Utf8PathBuf {
    path
}
