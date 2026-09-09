//! Walks one root in parallel and lexes every opted-in file.

use std::collections::{BTreeSet, HashSet};
use std::sync::mpsc;

use camino::{Utf8Path, Utf8PathBuf};
use ignore::{WalkBuilder, WalkState};

use crate::marker::MarkerPayload;
use crate::root::{FilePath, Root};
use crate::text::{AnalyzeError, Container, FileAnalyzer, FileScan, LanguageRegistry};
use crate::tree::{Entry, FileTree};

/// External roots contribute only anchors: their references are someone else's to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Full,
    AnchorsOnly,
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: FilePath,
    pub scan: FileScan,
}

/// A file that was selected for scanning but could not be checked. Always reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: FilePath,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum SkipReason {
    #[error("file is {bytes} bytes, over the {limit}-byte limit (`scan.max-file-bytes`)")]
    TooLarge { bytes: u64, limit: u64 },
    #[error("file is not valid UTF-8")]
    NotUtf8,
    #[error("could not read file: {message}")]
    Unreadable { message: String },
    #[error(transparent)]
    Analyze(AnalyzeError),
}

/// A walker error not attributable to a scannable file (permission denied on a directory, a
/// non-UTF-8 file name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkProblem {
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScanOutput {
    pub files: Vec<ScannedFile>,
    pub skipped: Vec<SkippedFile>,
    pub problems: Vec<WalkProblem>,
    /// Lowercased extensions of every file the walk reached, parsed or not. `[scan] include`
    /// narrows what is checked, not what exists, so it does not apply here.
    pub extensions: BTreeSet<String>,
    /// Every file and symlink the walk reached, whatever its container or include status: the
    /// repository as the scan sees it, which is what a reference target must exist in.
    pub tree: FileTree,
}

enum Outcome {
    File(ScannedFile),
    Skipped(SkippedFile),
    Problem(WalkProblem),
    Extension(String),
    Entry { path: FilePath, entry: Entry },
}

/// Respects `.gitignore` exactly as git does: only inside a git repository, with parent
/// `.gitignore` files stopping at the nearest `.git`. `[ignore] paths` is layered on top as a
/// prune filter, after the walker's own gitignore pass, so a `!` line there can never resurrect
/// a gitignored file. Never follows symlinks, recording each one with its link text instead.
/// Hidden files are walked like any other (`.claude/skills` is exactly the documentation this
/// tool exists for); `.git` is always pruned because its internals are never documentation and
/// their extensions would pollute the coverage extension table. Includes are a post-filter,
/// because a walker whitelist would silently un-ignore gitignored files.
pub fn scan_root(root: &Root, registry: &LanguageRegistry, mode: ScanMode) -> ScanOutput {
    let root_dir = root.dir.clone();
    let ignored_paths = root.config.ignore.paths.clone();
    let mut builder = WalkBuilder::new(root.dir.as_std_path());
    builder
        .hidden(false)
        .filter_entry(move |entry| {
            if entry.file_name() == ".git" {
                return false;
            }
            let Ok(relative) = entry.path().strip_prefix(root_dir.as_std_path()) else {
                return true;
            };
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            !ignored_paths.matched(relative, is_dir).is_ignore()
        })
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(true)
        .ignore(false)
        .follow_links(false);

    let (sender, receiver) = mpsc::channel::<Outcome>();
    builder.build_parallel().run(|| {
        let sender = sender.clone();
        let mut analyzer = FileAnalyzer::new(registry, root.config.scan.parse_budget);
        let mut seen_extensions: HashSet<String> = HashSet::new();
        Box::new(move |entry| {
            let outcome = match entry {
                Err(error) => Some(Outcome::Problem(WalkProblem {
                    path: None,
                    message: error.to_string(),
                })),
                Ok(entry) if entry.depth() == 0 => None,
                Ok(entry) => match relative_path(root, entry.path()) {
                    Err(problem) => Some(Outcome::Problem(problem)),
                    Ok(path) if entry.path_is_symlink() => Some(symlink_entry(path, &entry)),
                    Ok(path) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                        let _ = sender.send(Outcome::Entry {
                            path: path.clone(),
                            entry: Entry::File,
                        });
                        if mode == ScanMode::Full
                            && let Some(extension) = path.as_path().extension()
                            && seen_extensions.insert(extension.to_ascii_lowercase())
                        {
                            let _ = sender.send(Outcome::Extension(extension.to_ascii_lowercase()));
                        }
                        visit_file(root, &mut analyzer, mode, path, &entry)
                    }
                    Ok(_) => None,
                },
            };
            if let Some(outcome) = outcome {
                // The receiver outlives the walk; a send can only fail after it is dropped.
                let _ = sender.send(outcome);
            }
            WalkState::Continue
        })
    });
    drop(sender);

    let mut output = ScanOutput::default();
    for outcome in receiver {
        match outcome {
            Outcome::File(file) => output.files.push(file),
            Outcome::Skipped(skipped) => output.skipped.push(skipped),
            Outcome::Problem(problem) => output.problems.push(problem),
            Outcome::Extension(extension) => {
                output.extensions.insert(extension);
            }
            Outcome::Entry { path, entry } => output.tree.insert(&path, entry),
        }
    }
    output.files.sort_by(|a, b| a.path.cmp(&b.path));
    output.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    output
        .problems
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.message.cmp(&b.message)));
    output
}

fn relative_path(root: &Root, absolute: &std::path::Path) -> Result<FilePath, WalkProblem> {
    let problem = |message: String| WalkProblem {
        path: Some(absolute.to_string_lossy().into_owned()),
        message,
    };
    let relative = absolute
        .strip_prefix(root.dir.as_std_path())
        .map_err(|_| problem("walked path is outside the root".to_owned()))?;
    let relative = Utf8Path::from_path(relative)
        .ok_or_else(|| problem("file name is not valid UTF-8".to_owned()))?;
    FilePath::new(relative.to_path_buf()).map_err(|error| problem(error.to_string()))
}

/// A symlink is recorded with its link text and never followed; an unreadable one is a problem
/// rather than silently absent, because absence from the tree reads as "does not exist".
fn symlink_entry(path: FilePath, entry: &ignore::DirEntry) -> Outcome {
    match std::fs::read_link(entry.path()).map(Utf8PathBuf::from_path_buf) {
        Ok(Ok(target)) => Outcome::Entry {
            path,
            entry: Entry::Symlink { target },
        },
        Ok(Err(_)) => Outcome::Problem(WalkProblem {
            path: Some(path.to_string()),
            message: "symlink target is not valid UTF-8".to_owned(),
        }),
        Err(error) => Outcome::Problem(WalkProblem {
            path: Some(path.to_string()),
            message: format!("could not read symlink: {error}"),
        }),
    }
}

fn visit_file(
    root: &Root,
    analyzer: &mut FileAnalyzer<'_>,
    mode: ScanMode,
    path: FilePath,
    entry: &ignore::DirEntry,
) -> Option<Outcome> {
    let absolute = entry.path();
    if let Some(include) = &root.config.scan.include
        && !include.is_match(path.as_path())
    {
        return None;
    }
    let container =
        Container::for_path(path.as_path(), &root.config.containers, analyzer.registry())?;

    let skipped = |reason: SkipReason| {
        Some(Outcome::Skipped(SkippedFile {
            path: path.clone(),
            reason,
        }))
    };

    let limit = root.config.scan.max_file_bytes;
    let bytes = match entry.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return skipped(SkipReason::Unreadable {
                message: error.to_string(),
            });
        }
    };
    if bytes > limit {
        return skipped(SkipReason::TooLarge { bytes, limit });
    }

    let raw = match std::fs::read(absolute) {
        Ok(raw) => raw,
        Err(error) => {
            return skipped(SkipReason::Unreadable {
                message: error.to_string(),
            });
        }
    };
    let Ok(source) = String::from_utf8(raw) else {
        return skipped(SkipReason::NotUtf8);
    };

    match analyzer.scan(container, &source, &path) {
        Err(error) => skipped(SkipReason::Analyze(error)),
        Ok(mut scan) => {
            if mode == ScanMode::AnchorsOnly {
                scan.markers
                    .retain(|marker| matches!(marker.payload, MarkerPayload::Anchor { .. }));
                scan.malformed.clear();
            }
            Some(Outcome::File(ScannedFile { path, scan }))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
