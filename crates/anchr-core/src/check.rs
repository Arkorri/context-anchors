//! The one entrypoint: scan every present root, resolve every reference in the current root,
//! group what was found. Broken references are data in the report; only tool failures are
//! errors.

use std::collections::BTreeMap;

use crate::config::{ConfigError, Discovered, UnverifiedPolicy};
use crate::diagnostic::{
    DiagnosticKind, FileLocation, LocatedSite, Report, ReportBuilder, Summary,
};
use crate::index::{Index, Site};
use crate::resolve::{IndexedRoot, IndexedRoots, Resolution, Resolver};
use crate::root::{FilePath, RootName, RootSet, RootSetError};
use crate::scan::{ScanError, ScanMode, ScanOutput, scan_root};
use crate::span::PositionOverflow;
use crate::text::{LanguageRegistry, RegistryError};

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Overrides `[check] unverified` from config when set.
    pub unverified: Option<UnverifiedPolicy>,
    /// When non-empty, only references and malformed markers in these files are reported.
    /// Root-wide findings (duplicates, skipped files) are always reported.
    pub only_files: Vec<FilePath>,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Roots(#[from] RootSetError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("scanning root `{root}`: {source}")]
    Scan {
        root: RootName,
        #[source]
        source: ScanError,
    },
    #[error("a marker offset exceeded the addressable range: {0}")]
    Position(#[from] PositionOverflow),
}

pub fn run_check(discovered: Discovered, options: &CheckOptions) -> Result<Report, CheckError> {
    let registry = LanguageRegistry::new()?;
    let policy = options
        .unverified
        .unwrap_or(discovered.config.check.unverified);
    let root_set = RootSet::load(discovered.root_dir, discovered.config)?;

    let mut builder = ReportBuilder::default();
    let mut summary = Summary::default();
    let mut indexes = BTreeMap::new();
    let mut current_files = 0usize;

    for root in root_set.present() {
        let is_current = root.name == *root_set.current_name();
        let mode = if is_current {
            ScanMode::Full
        } else {
            ScanMode::AnchorsOnly
        };
        let output = scan_root(root, &registry, mode).map_err(|source| CheckError::Scan {
            root: root.name.clone(),
            source,
        })?;
        summary.roots_scanned += 1;
        if is_current {
            current_files = output.files.len();
        }
        record_scan_problems(&mut builder, &root.name, &output);
        indexes.insert(
            root.name.clone(),
            Index::from_scan(root.name.clone(), output.files),
        );
    }

    let roots = IndexedRoots::new(&root_set, indexes);
    let (current_root, current_index) = roots.current();
    summary.files_scanned = current_files;
    summary.anchors = current_index.anchor_count();

    let mut resolver = Resolver::new(&roots, &registry);
    let in_scope =
        |site: &Site| options.only_files.is_empty() || options.only_files.contains(&site.path);

    for reference in current_index.refs() {
        if !in_scope(&reference.site) {
            continue;
        }
        summary.refs_checked += 1;
        let located = locate(current_index, reference.site)?;
        match resolver.resolve(&current_root.name, reference.target) {
            Resolution::Resolved => summary.refs_resolved += 1,
            Resolution::Unresolved(unresolved) => {
                let suggestion = resolver.suggest(&unresolved);
                let kind = DiagnosticKind::Unresolved(unresolved);
                builder.suggestion(&kind, suggestion);
                builder.site(kind, located);
            }
            Resolution::Unverified(unverified) => {
                builder.site(DiagnosticKind::Unverified(unverified), located);
            }
        }
    }

    for malformed in current_index.malformed() {
        if !in_scope(&malformed.site) {
            continue;
        }
        let located = locate(current_index, malformed.site)?;
        builder.site(
            DiagnosticKind::Malformed {
                kind: malformed.malformed.kind,
                reason: malformed.malformed.reason.clone(),
            },
            located,
        );
    }

    for (id, sites) in current_index.duplicate_anchors() {
        let kind = DiagnosticKind::DuplicateAnchor {
            root: current_root.name.clone(),
            id: id.clone(),
        };
        for site in sites {
            let located = locate(current_index, site)?;
            builder.site(kind.clone(), located);
        }
    }

    for (name, entry) in roots.external() {
        let IndexedRoot::Present { index, .. } = entry else {
            continue;
        };
        for (id, sites) in index.duplicate_anchors() {
            let kind = DiagnosticKind::ExternalDuplicate {
                root: name.clone(),
                id: id.clone(),
            };
            for site in sites {
                let located = locate(index, site)?;
                builder.site(kind.clone(), located);
            }
        }
    }

    Ok(builder.finish(policy, summary))
}

fn record_scan_problems(builder: &mut ReportBuilder, root: &RootName, output: &ScanOutput) {
    for skipped in &output.skipped {
        builder.file(
            DiagnosticKind::FileSkipped {
                root: root.clone(),
                reason: skipped.reason.clone(),
            },
            FileLocation {
                root: root.clone(),
                path: skipped.path.clone(),
            },
        );
    }
    for problem in &output.problems {
        let message = match &problem.path {
            Some(path) => format!("{path}: {}", problem.message),
            None => problem.message.clone(),
        };
        builder.root(
            DiagnosticKind::WalkProblem {
                root: root.clone(),
                message,
            },
            root.clone(),
        );
    }
}

fn locate(index: &Index, site: Site) -> Result<LocatedSite, PositionOverflow> {
    let line_col = match index.line_index(&site.path) {
        Some(line_index) => line_index.line_col(site.span.start)?,
        // The site came from this index, so its file is present; this arm is unreachable in
        // practice and reported as an overflow rather than swallowed.
        None => {
            return Err(PositionOverflow {
                offset: site.span.start,
            });
        }
    };
    Ok(LocatedSite { site, line_col })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;

    use super::*;
    use crate::config;
    use crate::diagnostic::{Locations, Severity};
    use crate::resolve::{Unresolved, Unverified};

    struct Fixture {
        _dir: tempfile::TempDir,
        root_dir: Utf8PathBuf,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
            let root_dir = base.join("repo");
            for (path, contents) in files {
                let full = root_dir.join(path);
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(full, contents).unwrap();
            }
            Self {
                _dir: dir,
                root_dir,
            }
        }

        fn check(&self, options: &CheckOptions) -> Report {
            let discovered = config::discover(&self.root_dir).unwrap();
            run_check(discovered, options).unwrap()
        }
    }

    fn titles(report: &Report) -> Vec<String> {
        report
            .diagnostics
            .iter()
            .map(|d| d.kind.to_string())
            .collect()
    }

    #[test]
    fn a_clean_root_has_no_diagnostics_and_a_full_summary() {
        let fixture = Fixture::new(&[
            (
                "README.md",
                "# Repo @anchor[readme]\n\nSee @ref[#readme], @ref[src/lib.rs#run], @ref[src/].",
            ),
            ("src/lib.rs", "// Entry: @ref[#readme]\npub fn run() {}"),
        ]);
        let report = fixture.check(&CheckOptions::default());
        assert!(report.diagnostics.is_empty(), "{:?}", titles(&report));
        assert_eq!(report.summary.files_scanned, 2);
        assert_eq!(report.summary.anchors, 1);
        assert_eq!(report.summary.refs_checked, 4);
        assert_eq!(report.summary.refs_resolved, 4);
        assert!(!report.has_errors());
    }

    #[test]
    fn broken_references_group_by_cause_with_every_site() {
        let fixture = Fixture::new(&[
            ("a.md", "@ref[#gone] @ref[#gone]\n@ref[#gone]"),
            ("b.md", "@ref[#gone] @anchor[goner]"),
            ("src/x.rs", "// @ref[#gone]\nfn f() {}"),
        ]);
        let report = fixture.check(&CheckOptions::default());
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert!(matches!(
            diagnostic.kind,
            DiagnosticKind::Unresolved(Unresolved::AnchorMissing { .. })
        ));
        assert_eq!(diagnostic.locations.len(), 5);
        assert_eq!(diagnostic.suggestion.as_deref(), Some("goner"));
        assert_eq!(diagnostic.severity, Severity::Error);
        let Locations::Sites(sites) = &diagnostic.locations else {
            panic!("expected sites");
        };
        assert_eq!(sites[0].site.path.as_str(), "a.md");
        assert_eq!((sites[0].line_col.line, sites[0].line_col.col), (1, 1));
        assert_eq!((sites[2].line_col.line, sites[2].line_col.col), (2, 1));
        assert!(report.has_errors());
    }

    #[test]
    fn duplicates_malformed_and_skipped_files_are_reported() {
        let fixture = Fixture::new(&[
            ("a.md", "@anchor[dup] @ref[ @ref[a b]"),
            ("b.md", "@anchor[dup]"),
        ]);
        fs::write(fixture.root_dir.join("bin.md"), [0xff, 0xfe]).unwrap();
        let report = fixture.check(&CheckOptions::default());
        let kinds: Vec<&DiagnosticKind> = report.diagnostics.iter().map(|d| &d.kind).collect();
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, DiagnosticKind::DuplicateAnchor { .. }))
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| matches!(k, DiagnosticKind::Malformed { .. }))
                .count(),
            2
        );
        assert!(kinds.iter().any(|k| matches!(
            k,
            DiagnosticKind::FileSkipped {
                reason: crate::scan::SkipReason::NotUtf8,
                ..
            }
        )));
        assert_eq!(report.summary.errors, 3);
        assert_eq!(report.summary.unverified, 1);
    }

    #[test]
    fn absent_roots_are_unverified_unless_strict() {
        let fixture = Fixture::new(&[
            ("anchr.toml", "[roots]\nclaude = \"../not-there\"\n"),
            ("a.md", "@ref[claude:#x] @ref[claud:#x]"),
        ]);
        let report = fixture.check(&CheckOptions::default());
        assert_eq!(report.diagnostics.len(), 2);
        assert!(matches!(
            report.diagnostics[0].kind,
            DiagnosticKind::Unresolved(Unresolved::RootUndeclared { .. })
        ));
        assert_eq!(report.diagnostics[0].suggestion.as_deref(), Some("claude"));
        assert!(matches!(
            report.diagnostics[1].kind,
            DiagnosticKind::Unverified(Unverified::RootAbsent { .. })
        ));
        assert_eq!(report.diagnostics[1].severity, Severity::Unverified);
        assert_eq!(report.summary.errors, 1);

        let strict = fixture.check(&CheckOptions {
            unverified: Some(UnverifiedPolicy::Error),
            ..CheckOptions::default()
        });
        assert_eq!(strict.summary.errors, 2);
    }

    #[test]
    fn external_roots_contribute_anchors_and_their_duplicates_are_unverified() {
        let dir = tempfile::tempdir().unwrap();
        let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let repo = base.join("repo");
        let plugin = base.join("plugin");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&plugin).unwrap();
        fs::write(repo.join("anchr.toml"), "[roots]\nplugin = \"../plugin\"\n").unwrap();
        fs::write(repo.join("a.md"), "@ref[plugin:#skill] @ref[plugin:#twice]").unwrap();
        fs::write(
            plugin.join("SKILL.md"),
            "@anchor[skill] @anchor[twice] @anchor[twice] @ref[#nonexistent-but-not-ours] @ref[",
        )
        .unwrap();

        let report = run_check(config::discover(&repo).unwrap(), &CheckOptions::default()).unwrap();
        assert_eq!(titles(&report).len(), 1, "{:?}", titles(&report));
        assert!(matches!(
            report.diagnostics[0].kind,
            DiagnosticKind::ExternalDuplicate { .. }
        ));
        assert_eq!(report.diagnostics[0].severity, Severity::Unverified);
        assert_eq!(report.summary.refs_resolved, 2);
        assert_eq!(report.summary.roots_scanned, 2);
    }

    #[test]
    fn only_files_filters_references_but_not_root_wide_findings() {
        let fixture = Fixture::new(&[
            ("a.md", "@ref[#gone] @anchor[dup]"),
            ("b.md", "@ref[#gone] @anchor[dup]"),
        ]);
        let report = fixture.check(&CheckOptions {
            only_files: vec![FilePath::new(Utf8PathBuf::from("a.md")).unwrap()],
            ..CheckOptions::default()
        });
        let missing = report
            .diagnostics
            .iter()
            .find(|d| matches!(d.kind, DiagnosticKind::Unresolved(_)))
            .unwrap();
        assert_eq!(missing.locations.len(), 1);
        let duplicate = report
            .diagnostics
            .iter()
            .find(|d| matches!(d.kind, DiagnosticKind::DuplicateAnchor { .. }))
            .unwrap();
        assert_eq!(duplicate.locations.len(), 2);
        assert_eq!(report.summary.refs_checked, 1);
    }
}
