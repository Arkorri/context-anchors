//! Walks one root in parallel and lexes every opted-in file.

use std::sync::mpsc;

use camino::Utf8Path;
use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkState};

use crate::marker::MarkerPayload;
use crate::root::{FilePath, Root};
use crate::text::{AnalyzeError, Container, FileAnalyzer, FileScan, LanguageRegistry};

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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("invalid exclude pattern: {0}")]
    Overrides(#[source] ignore::Error),
}

enum Outcome {
    File(ScannedFile),
    Skipped(SkippedFile),
    Problem(WalkProblem),
}

/// Respects `.gitignore` (with or without a `.git` directory), `.ignore`, and `.anchrignore`;
/// never follows symlinks. Excludes are walker overrides; includes are a post-filter, because
/// an include override would silently un-ignore gitignored files.
pub fn scan_root(
    root: &Root,
    registry: &LanguageRegistry,
    mode: ScanMode,
) -> Result<ScanOutput, ScanError> {
    let mut overrides = OverrideBuilder::new(root.dir.as_std_path());
    for pattern in &root.config.scan.exclude_patterns {
        overrides
            .add(&format!("!{pattern}"))
            .map_err(ScanError::Overrides)?;
    }
    let overrides = overrides.build().map_err(ScanError::Overrides)?;

    let mut builder = WalkBuilder::new(root.dir.as_std_path());
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .add_custom_ignore_filename(".anchrignore")
        .overrides(overrides);

    let (sender, receiver) = mpsc::channel::<Outcome>();
    builder.build_parallel().run(|| {
        let sender = sender.clone();
        let mut analyzer = FileAnalyzer::new(registry, root.config.scan.parse_budget);
        Box::new(move |entry| {
            let outcome = match entry {
                Err(error) => Some(Outcome::Problem(WalkProblem {
                    path: None,
                    message: error.to_string(),
                })),
                Ok(entry) => {
                    if entry.file_type().is_some_and(|kind| kind.is_file()) {
                        visit_file(root, &mut analyzer, mode, entry.path(), &entry)
                    } else {
                        None
                    }
                }
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
        }
    }
    output.files.sort_by(|a, b| a.path.cmp(&b.path));
    output.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    output
        .problems
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.message.cmp(&b.message)));
    Ok(output)
}

fn visit_file(
    root: &Root,
    analyzer: &mut FileAnalyzer<'_>,
    mode: ScanMode,
    absolute: &std::path::Path,
    entry: &ignore::DirEntry,
) -> Option<Outcome> {
    let relative = absolute.strip_prefix(root.dir.as_std_path()).ok()?;
    let Some(relative) = Utf8Path::from_path(relative) else {
        return Some(Outcome::Problem(WalkProblem {
            path: Some(relative.to_string_lossy().into_owned()),
            message: "file name is not valid UTF-8".to_owned(),
        }));
    };
    let path = FilePath::new(relative.to_path_buf()).ok()?;

    if let Some(include) = &root.config.scan.include
        && !include.is_match(relative)
    {
        return None;
    }
    let container = Container::for_path(relative, &root.config.containers, analyzer.registry())?;

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

    match analyzer.scan(container, &source) {
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
mod tests {
    use camino::Utf8PathBuf;

    use super::*;
    use crate::config::Config;
    use crate::root::RootName;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: Root,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            Self::with_config(files, Config::default())
        }

        fn with_config(files: &[(&str, &str)], config: Config) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
            for (path, contents) in files {
                let full = base.join(path);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, contents).unwrap();
            }
            Self {
                _dir: dir,
                root: Root {
                    name: RootName::parse("fixture").unwrap(),
                    dir: base,
                    config,
                },
            }
        }

        fn scan(&self, mode: ScanMode) -> ScanOutput {
            let registry = LanguageRegistry::new().unwrap();
            scan_root(&self.root, &registry, mode).unwrap()
        }
    }

    fn paths(files: &[ScannedFile]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    #[test]
    fn scans_opted_in_containers_and_ignores_everything_else() {
        let fixture = Fixture::new(&[
            ("README.md", "@anchor[readme]"),
            ("notes.txt", "@ref[README.md]"),
            ("src/lib.rs", "// @ref[#readme]"),
            ("Makefile", "@ref[not-scanned]"),
            ("image.png", "@ref[not-scanned]"),
        ]);
        let output = fixture.scan(ScanMode::Full);
        assert_eq!(
            paths(&output.files),
            vec!["README.md", "notes.txt", "src/lib.rs"]
        );
        assert!(output.skipped.is_empty());
        assert!(output.problems.is_empty());
        let total: usize = output.files.iter().map(|f| f.scan.markers.len()).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn gitignore_is_honoured_and_include_cannot_override_it() {
        let mut config = Config::default();
        config.scan.include = Some(
            globset::GlobSetBuilder::new()
                .add(globset::Glob::new("**/*.md").unwrap())
                .build()
                .unwrap(),
        );
        let fixture = Fixture::with_config(
            &[
                (".gitignore", "generated.md\n"),
                ("kept.md", ""),
                ("generated.md", ""),
                ("notes.txt", "not included"),
            ],
            config,
        );
        assert_eq!(paths(&fixture.scan(ScanMode::Full).files), vec!["kept.md"]);
    }

    #[test]
    fn exclude_patterns_and_anchrignore_prune_files() {
        let mut config = Config::default();
        config.scan.exclude_patterns = vec!["vendor/**".to_owned()];
        let fixture = Fixture::with_config(
            &[
                (".anchrignore", "drafts/\n"),
                ("kept.md", ""),
                ("vendor/lib.md", ""),
                ("drafts/wip.md", ""),
            ],
            config,
        );
        assert_eq!(paths(&fixture.scan(ScanMode::Full).files), vec!["kept.md"]);
    }

    #[test]
    fn oversized_and_non_utf8_files_are_skipped_with_a_reason() {
        let mut config = Config::default();
        config.scan.max_file_bytes = 16;
        let fixture = Fixture::with_config(
            &[
                ("small.md", "@anchor[a]"),
                ("big.md", "x".repeat(17).as_str()),
            ],
            config,
        );
        std::fs::write(fixture.root.dir.join("binary.md"), [0xff, 0xfe, b'@']).unwrap();

        let output = fixture.scan(ScanMode::Full);
        assert_eq!(paths(&output.files), vec!["small.md"]);
        let reasons: Vec<(&str, &SkipReason)> = output
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), &s.reason))
            .collect();
        assert!(matches!(
            reasons[0],
            (
                "big.md",
                SkipReason::TooLarge {
                    bytes: 17,
                    limit: 16
                }
            )
        ));
        assert!(matches!(reasons[1], ("binary.md", SkipReason::NotUtf8)));
    }

    #[test]
    fn files_with_unreferenceable_names_are_still_scanned() {
        let fixture = Fixture::new(&[("My Notes #1.md", "@anchor[notes]")]);
        let output = fixture.scan(ScanMode::Full);
        assert_eq!(paths(&output.files), vec!["My Notes #1.md"]);
        assert_eq!(output.files[0].scan.markers.len(), 1);
    }

    #[test]
    fn anchors_only_mode_drops_references_and_malformed_markers() {
        let fixture = Fixture::new(&[("a.md", "@anchor[a] @ref[#b] @ref[")]);
        let full = fixture.scan(ScanMode::Full);
        assert_eq!(full.files[0].scan.markers.len(), 2);
        assert_eq!(full.files[0].scan.malformed.len(), 1);

        let anchors_only = fixture.scan(ScanMode::AnchorsOnly);
        assert_eq!(anchors_only.files[0].scan.markers.len(), 1);
        assert!(anchors_only.files[0].scan.malformed.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "@anchor[secret]").unwrap();
        let fixture = Fixture::new(&[("kept.md", "")]);
        std::os::unix::fs::symlink(outside.path(), fixture.root.dir.join("linked")).unwrap();
        assert_eq!(paths(&fixture.scan(ScanMode::Full).files), vec!["kept.md"]);
    }
}
