use std::fs;

use super::*;
use crate::config;
use crate::diagnostic::{Diagnostic, Locations, Severity};
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
        "@anchor[skill] @anchor[twice] @anchor[twice] @ref[#nonexistent-but-not-ours] @ref[ @[Nope]",
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

#[test]
fn alias_uses_bind_in_their_file_and_report_undeclared_and_duplicates() {
    let fixture = Fixture::new(&[
        (
            "a.md",
            "@anchor[x]\n@ref[#x as X] @[X] @[X] @[Y]\n@ref[#x as Z] @ref[#x as Z] @[Z]\n",
        ),
        ("b.md", "@ref[#nope as Nope] @[Nope] @[X]\n"),
    ]);
    let report = fixture.check(&CheckOptions::default());
    assert_eq!(report.summary.refs_checked, 4);
    assert_eq!(report.summary.refs_resolved, 3);
    assert_eq!(report.summary.alias_uses, 4, "{:?}", titles(&report));

    let undeclared: Vec<&Diagnostic> = report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.kind, DiagnosticKind::AliasUndeclared { .. }))
        .collect();
    assert_eq!(undeclared.len(), 2, "{:?}", titles(&report));
    let in_a = undeclared
        .iter()
        .find(|d| matches!(&d.kind, DiagnosticKind::AliasUndeclared { path, .. } if path.as_str() == "a.md"))
        .unwrap();
    assert!(in_a.suggestion.is_some());
    assert_eq!(in_a.locations.len(), 1);
    let in_b = undeclared
        .iter()
        .find(|d| matches!(&d.kind, DiagnosticKind::AliasUndeclared { path, .. } if path.as_str() == "b.md"))
        .unwrap();
    assert_eq!(in_b.suggestion, None);

    let duplicate = report
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, DiagnosticKind::AliasDuplicate { .. }))
        .unwrap();
    assert_eq!(duplicate.locations.len(), 2);

    let missing = report
        .diagnostics
        .iter()
        .find(|d| {
            matches!(
                d.kind,
                DiagnosticKind::Unresolved(Unresolved::AnchorMissing { .. })
            )
        })
        .unwrap();
    assert_eq!(missing.locations.len(), 1);
    assert_eq!(
        missing.notes,
        vec!["alias `Nope` has 1 use in `b.md`".to_owned()]
    );
    assert!(report.has_errors());

    let scoped = fixture.check(&CheckOptions {
        only_files: vec![FilePath::new(Utf8PathBuf::from("a.md")).unwrap()],
        ..CheckOptions::default()
    });
    assert_eq!(scoped.summary.alias_uses, 3);
    assert_eq!(
        titles(&scoped)
            .iter()
            .filter(|t| t.contains("b.md"))
            .count(),
        0,
        "{:?}",
        titles(&scoped)
    );
}
