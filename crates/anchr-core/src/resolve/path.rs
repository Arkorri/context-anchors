use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::marker::RelPath;
use crate::root::{Root, RootName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    /// A symlink whose target is missing, or an entry of another type (socket, device).
    Other,
}

/// The names in one directory, as the filesystem reports them: exact bytes, no case folding.
#[derive(Debug, Default)]
struct Listing {
    entries: HashMap<String, EntryKind>,
}

pub(super) enum Located {
    Found {
        kind: EntryKind,
        absolute: Utf8PathBuf,
    },
    Missing {
        /// The directory in which lookup failed, and the name that was not there.
        parent: Utf8PathBuf,
        name: String,
    },
}

/// Existence by exact-name directory listing rather than `metadata()`, so that
/// `@ref[src/Foo.ts]` fails the same way on a case-insensitive filesystem as in Linux CI.
#[derive(Default)]
pub(super) struct PathResolver {
    listings: HashMap<Utf8PathBuf, Option<Listing>>,
    canonical_roots: HashMap<RootName, Option<Utf8PathBuf>>,
}

impl PathResolver {
    pub(super) fn locate(&mut self, root: &Root, path: &RelPath) -> Located {
        let mut current = root.dir.clone();
        let mut components = path.as_path().components().peekable();
        while let Some(component) = components.next() {
            let name = component.as_str();
            let Some(kind) = self.entry_kind(&current, name) else {
                return Located::Missing {
                    parent: current,
                    name: name.to_owned(),
                };
            };
            current.push(name);
            if components.peek().is_some() && kind != EntryKind::Directory {
                let next = components
                    .next()
                    .map(|c| c.as_str().to_owned())
                    .unwrap_or_default();
                return Located::Missing {
                    parent: current,
                    name: next,
                };
            }
            if components.peek().is_none() {
                return Located::Found {
                    kind,
                    absolute: current,
                };
            }
        }
        Located::Found {
            kind: EntryKind::Directory,
            absolute: current,
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
    pub(super) fn suggest(&mut self, root: &Root, path: &RelPath) -> Option<String> {
        let Located::Missing { parent, name } = self.locate(root, path) else {
            return None;
        };
        let listing = self.listing(&parent)?;
        let candidate = crate::suggest::suggest(&name, listing.entries.keys().map(String::as_str))?;
        let prefix = parent.strip_prefix(&root.dir).ok()?;
        let suggested = if prefix.as_str().is_empty() {
            candidate
        } else {
            format!("{prefix}/{candidate}")
        };
        Some(suggested)
    }

    fn entry_kind(&mut self, dir: &Utf8Path, name: &str) -> Option<EntryKind> {
        self.listing(dir)?.entries.get(name).copied()
    }

    fn listing(&mut self, dir: &Utf8Path) -> Option<&Listing> {
        if !self.listings.contains_key(dir) {
            let listing = read_listing(dir);
            self.listings.insert(dir.to_path_buf(), listing);
        }
        self.listings.get(dir).and_then(Option::as_ref)
    }
}

fn read_listing(dir: &Utf8Path) -> Option<Listing> {
    let read = fs::read_dir(dir).ok()?;
    let mut listing = Listing::default();
    for entry in read.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let kind = classify(&entry);
        listing.entries.insert(name, kind);
    }
    Some(listing)
}

/// A symlink is classified by what it points at, so `docs -> ../shared-docs` is a directory
/// for existence purposes; reads still go through the containment check.
fn classify(entry: &fs::DirEntry) -> EntryKind {
    let Ok(file_type) = entry.file_type() else {
        return EntryKind::Other;
    };
    if file_type.is_symlink() {
        return match fs::metadata(entry.path()) {
            Ok(target) if target.is_dir() => EntryKind::Directory,
            Ok(target) if target.is_file() => EntryKind::File,
            _ => EntryKind::Other,
        };
    }
    if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
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
