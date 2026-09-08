//! What one scan enumerated: every file and symlink, keyed by exact root-relative path.
//!
//! Existence is decided here, not on the disk, so that a local run and a clean checkout agree.
//! A gitignored build artifact, an excluded tree, or an empty directory is present on this
//! machine and absent from the repository, and only the second fact matters to a reference.
//! Keys are the bytes the walker reported, so a reference never matches a differently cased
//! file on a case-insensitive filesystem.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

use crate::root::FilePath;

/// Symlink chains longer than this are missing rather than followed further.
pub const MAX_SYMLINK_HOPS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    File,
    /// The link text as `read_link` returned it, resolved lexically at lookup and never
    /// followed on disk.
    Symlink {
        target: Utf8PathBuf,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FileTree {
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    File,
    /// At least one enumerated entry lies beneath the path.
    Directory,
    /// `parent` is the deepest existing prefix (`""` for the root) and `name` the component
    /// that was not found in it.
    Missing {
        parent: String,
        name: String,
    },
    /// Reached through a symlink whose target leaves the root.
    LeavesRoot,
}

impl FileTree {
    pub fn insert(&mut self, path: &FilePath, entry: Entry) {
        self.entries.insert(path.as_str().to_owned(), entry);
    }

    pub fn remove(&mut self, path: &FilePath) {
        self.entries.remove(path.as_str());
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn lookup(&self, path: &Utf8Path) -> Lookup {
        let mut current = path.to_path_buf();
        for _ in 0..=MAX_SYMLINK_HOPS {
            if matches!(self.entries.get(current.as_str()), Some(Entry::File)) {
                return Lookup::File;
            }
            if self.is_dir(current.as_str()) {
                return Lookup::Directory;
            }
            let Some((link, target)) = self.symlink_prefix(&current) else {
                return self.missing(&current);
            };
            let rest = current.strip_prefix(link).unwrap_or(Utf8Path::new(""));
            let base = link.parent().unwrap_or(Utf8Path::new(""));
            match normalize(&base.join(target).join(rest)) {
                Some(redirected) => current = redirected,
                None => return Lookup::LeavesRoot,
            }
        }
        self.missing(path)
    }

    /// Distinct first components beneath `dir` (`""` for the root): files, symlinks, and
    /// directory names alike.
    pub fn children(&self, dir: &str) -> BTreeSet<&str> {
        let prefix = slashed(dir);
        self.entries
            .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
            .take_while(|(key, _)| key.starts_with(prefix.as_str()))
            .map(|(key, _)| {
                let rest = &key[prefix.len()..];
                rest.split('/').next().unwrap_or(rest)
            })
            .collect()
    }

    /// Every entry whose final component is `name`. A linear scan, run only for paths that
    /// are already known to be missing.
    pub fn by_basename(&self, name: &str) -> Vec<&str> {
        self.entries
            .keys()
            .filter(|key| key.rsplit('/').next() == Some(name))
            .map(String::as_str)
            .collect()
    }

    fn is_dir(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return !self.entries.is_empty();
        }
        let prefix = slashed(prefix);
        self.entries
            .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
            .next()
            .is_some_and(|(key, _)| key.starts_with(prefix.as_str()))
    }

    /// The longest recorded symlink that is `path` or one of its ancestors.
    fn symlink_prefix<'a>(&'a self, path: &'a Utf8Path) -> Option<(&'a Utf8Path, &'a Utf8Path)> {
        path.ancestors()
            .filter(|ancestor| !ancestor.as_str().is_empty())
            .find_map(|ancestor| match self.entries.get(ancestor.as_str()) {
                Some(Entry::Symlink { target }) => Some((ancestor, target.as_path())),
                _ => None,
            })
    }

    fn missing(&self, path: &Utf8Path) -> Lookup {
        let mut parent = String::new();
        let mut components = path.components().map(|c| c.as_str()).peekable();
        while let Some(name) = components.next() {
            let candidate = if parent.is_empty() {
                name.to_owned()
            } else {
                format!("{parent}/{name}")
            };
            if self.is_dir(&candidate) {
                parent = candidate;
                continue;
            }
            if components.peek().is_some()
                && matches!(self.entries.get(candidate.as_str()), Some(Entry::File))
            {
                return Lookup::Missing {
                    parent: candidate,
                    name: components.next().unwrap_or_default().to_owned(),
                };
            }
            return Lookup::Missing {
                parent,
                name: name.to_owned(),
            };
        }
        Lookup::Missing {
            parent,
            name: String::new(),
        }
    }
}

fn slashed(dir: &str) -> String {
    if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    }
}

/// Lexical `.` and `..` collapse. `None` when the path is absolute or climbs above its start;
/// no filesystem access, so it also works for paths that do not exist. Segments are joined
/// with `/` rather than pushed, so the result matches tree keys on Windows too.
pub fn normalize(candidate: &Utf8Path) -> Option<Utf8PathBuf> {
    let mut segments: Vec<&str> = Vec::new();
    for component in candidate.components() {
        match component {
            Utf8Component::Normal(segment) => segments.push(segment),
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                segments.pop()?;
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => return None,
        }
    }
    Some(Utf8PathBuf::from(segments.join("/")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tree(files: &[&str], links: &[(&str, &str)]) -> FileTree {
        let mut tree = FileTree::default();
        for file in files {
            tree.insert(
                &FilePath::new(Utf8PathBuf::from(file)).unwrap(),
                Entry::File,
            );
        }
        for (link, target) in links {
            tree.insert(
                &FilePath::new(Utf8PathBuf::from(link)).unwrap(),
                Entry::Symlink {
                    target: Utf8PathBuf::from(target),
                },
            );
        }
        tree
    }

    fn missing(parent: &str, name: &str) -> Lookup {
        Lookup::Missing {
            parent: parent.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn files_directories_and_prefixes_are_told_apart_by_exact_bytes() {
        let t = tree(&["docs/guide.md", "docs2/x.md", "Cargo.lock"], &[]);
        assert_eq!(t.lookup("docs/guide.md".into()), Lookup::File);
        assert_eq!(t.lookup("Cargo.lock".into()), Lookup::File);
        assert_eq!(t.lookup("docs".into()), Lookup::Directory);
        assert_eq!(t.lookup("".into()), Lookup::Directory);
        assert_eq!(t.lookup("doc".into()), missing("", "doc"));
        assert_eq!(t.lookup("docs2/y.md".into()), missing("docs2", "y.md"));
        assert_eq!(
            t.lookup("docs/guide.md/x".into()),
            missing("docs/guide.md", "x")
        );
        assert_eq!(t.lookup("Docs/guide.md".into()), missing("", "Docs"));
        assert_eq!(t.lookup("docs/sub/deep.md".into()), missing("docs", "sub"));
        assert!(FileTree::default().lookup("".into()) != Lookup::Directory);
    }

    #[test]
    fn children_and_basenames_come_from_the_tree() {
        let t = tree(
            &["docs/guide.md", "docs/sub/x.md", "src/lib.rs", "guide.md"],
            &[("docs/link", "sub")],
        );
        assert_eq!(
            t.children("docs"),
            ["guide.md", "link", "sub"].into_iter().collect()
        );
        assert_eq!(
            t.children(""),
            ["docs", "guide.md", "src"].into_iter().collect()
        );
        assert_eq!(t.children("nowhere"), BTreeSet::new());
        assert_eq!(t.by_basename("guide.md"), vec!["docs/guide.md", "guide.md"]);
        assert_eq!(t.by_basename("x.md"), vec!["docs/sub/x.md"]);
        assert!(t.by_basename("nope.md").is_empty());
    }

    #[test]
    fn symlinks_redirect_lexically_and_never_leave_the_root() {
        let t = tree(
            &["docs/guide.md", "c.md"],
            &[
                ("a", "b"),
                ("b", "c.md"),
                ("loop1", "loop2"),
                ("loop2", "loop1"),
                ("up", "../x"),
                ("abs", "/etc"),
                ("dangling", "target/x.md"),
                ("link", "docs"),
                ("CLAUDE.md", "AGENTS.md"),
            ],
        );
        assert_eq!(t.lookup("a".into()), Lookup::File);
        assert_eq!(t.lookup("link".into()), Lookup::Directory);
        assert_eq!(t.lookup("link/guide.md".into()), Lookup::File);
        assert_eq!(t.lookup("link/nope.md".into()), missing("docs", "nope.md"));
        assert_eq!(t.lookup("loop1".into()), missing("", "loop1"));
        assert_eq!(t.lookup("up".into()), Lookup::LeavesRoot);
        assert_eq!(t.lookup("up/deeper.md".into()), Lookup::LeavesRoot);
        assert_eq!(t.lookup("abs".into()), Lookup::LeavesRoot);
        assert_eq!(t.lookup("dangling".into()), missing("", "target"));
        assert_eq!(t.lookup("CLAUDE.md".into()), missing("", "AGENTS.md"));
    }

    #[test]
    fn normalize_collapses_dots_and_rejects_escapes() {
        let n = |s: &str| normalize(Utf8Path::new(s)).map(|p| p.to_string());
        assert_eq!(n("docs/./guide.md"), Some("docs/guide.md".to_owned()));
        assert_eq!(n("docs/sub/../guide.md"), Some("docs/guide.md".to_owned()));
        assert_eq!(n("docs/.."), Some(String::new()));
        assert_eq!(n("a/b/../../c"), Some("c".to_owned()));
        assert_eq!(n(".."), None);
        assert_eq!(n("docs/../../x"), None);
        assert_eq!(n("/etc/passwd"), None);
    }
}
