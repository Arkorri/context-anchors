use camino::Utf8PathBuf;

use super::*;
use crate::span::ByteSpan;
use crate::text::RegionKind;

fn root() -> RootName {
    RootName::parse("r").unwrap()
}

fn located(path: &str, line: u32) -> LocatedSite {
    LocatedSite {
        site: Site {
            root: root(),
            path: FilePath::new(Utf8PathBuf::from(path)).unwrap(),
            span: ByteSpan::new(0, 5),
            region: RegionKind::Prose,
        },
        line_col: LineCol { line, col: 1 },
    }
}

fn missing(id: &str) -> DiagnosticKind {
    DiagnosticKind::Unresolved(Unresolved::AnchorMissing {
        root: root(),
        id: AnchorId::parse(id).unwrap(),
    })
}

#[test]
fn sites_group_by_cause_and_sort_by_location() {
    let mut builder = ReportBuilder::default();
    builder.site(missing("auth/flow"), located("z.md", 3));
    builder.site(missing("auth/flow"), located("a.md", 9));
    builder.suggestion(&missing("auth/flow"), Some("auth/token-refresh".to_owned()));
    builder.site(missing("auth/flow"), located("a.md", 2));
    builder.site(missing("other"), located("b.md", 1));

    let report = builder.finish(
        UnverifiedPolicy::Report,
        Summary::default(),
        BTreeMap::new(),
    );
    assert_eq!(report.diagnostics.len(), 2);
    let first = &report.diagnostics[0];
    assert_eq!(first.kind, missing("auth/flow"));
    assert_eq!(first.suggestion.as_deref(), Some("auth/token-refresh"));
    let Locations::Sites(sites) = &first.locations else {
        panic!("expected sites");
    };
    let order: Vec<(String, u32)> = sites
        .iter()
        .map(|s| (s.site.path.to_string(), s.line_col.line))
        .collect();
    assert_eq!(
        order,
        vec![
            ("a.md".to_owned(), 2),
            ("a.md".to_owned(), 9),
            ("z.md".to_owned(), 3)
        ]
    );
    assert_eq!(report.summary.errors, 2);
    assert!(report.has_errors());
}

#[test]
fn errors_sort_before_unverified_and_strict_promotes_them() {
    let unverified = DiagnosticKind::Unverified(Unverified::RootAbsent {
        name: RootName::parse("claude").unwrap(),
        declared_dir: Utf8PathBuf::from("/x"),
    });
    let mut builder = ReportBuilder::default();
    builder.site(unverified.clone(), located("a.md", 1));
    builder.site(unverified.clone(), located("a.md", 2));
    builder.site(missing("x"), located("b.md", 1));

    let lenient = builder.finish(
        UnverifiedPolicy::Report,
        Summary::default(),
        BTreeMap::new(),
    );
    assert_eq!(lenient.diagnostics[0].kind, missing("x"));
    assert_eq!(lenient.diagnostics[1].severity, Severity::Unverified);
    assert_eq!((lenient.summary.errors, lenient.summary.unverified), (1, 1));
    assert!(lenient.diagnostics[1].kind.hint().is_some());

    let mut builder = ReportBuilder::default();
    builder.site(unverified, located("a.md", 1));
    let strict = builder.finish(UnverifiedPolicy::Error, Summary::default(), BTreeMap::new());
    assert_eq!(strict.diagnostics[0].severity, Severity::Error);
    assert!(strict.has_errors());
}

#[test]
fn file_and_root_level_findings_have_their_own_location_shapes() {
    let mut builder = ReportBuilder::default();
    let skipped = DiagnosticKind::FileSkipped {
        root: root(),
        reason: SkipReason::NotUtf8,
    };
    builder.file(
        skipped,
        FileLocation {
            root: root(),
            path: FilePath::new(Utf8PathBuf::from("bin.md")).unwrap(),
        },
    );
    let walk = DiagnosticKind::WalkProblem {
        root: root(),
        message: "permission denied".to_owned(),
    };
    builder.root(walk, root());
    let report = builder.finish(
        UnverifiedPolicy::Report,
        Summary::default(),
        BTreeMap::new(),
    );
    let skipped_diagnostic = report
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, DiagnosticKind::FileSkipped { .. }))
        .unwrap();
    assert!(matches!(skipped_diagnostic.locations, Locations::Files(ref f) if f.len() == 1));
    assert!(skipped_diagnostic.kind.hint().is_some());
    let walk_diagnostic = report
        .diagnostics
        .iter()
        .find(|d| matches!(d.kind, DiagnosticKind::WalkProblem { .. }))
        .unwrap();
    assert!(matches!(walk_diagnostic.locations, Locations::Roots(ref r) if r.len() == 1));
}

#[test]
fn every_kind_has_a_human_title() {
    let title = missing("auth/flow").to_string();
    assert_eq!(title, "unknown anchor id `auth/flow` in root `r`");
    let malformed = DiagnosticKind::Malformed {
        kind: MarkerKind::Ref,
        reason: MalformedReason::Unclosed,
    };
    assert_eq!(
        malformed.to_string(),
        "malformed @ref: missing closing `]` on the same line"
    );
    let undeclared = DiagnosticKind::AliasUndeclared {
        root: RootName::parse("r").unwrap(),
        path: FilePath::new(camino::Utf8PathBuf::from("docs/x.md")).unwrap(),
        alias: Alias::parse("Analyser").unwrap(),
    };
    assert_eq!(
        undeclared.to_string(),
        "alias `Analyser` is not declared in `docs/x.md` (root `r`)"
    );
    assert_eq!(undeclared.base_severity(), Severity::Error);
    assert!(undeclared.hint().unwrap().contains("as Analyser]"));
}
