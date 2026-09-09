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

/// Every kind, so a new variant fails to compile here until its wording is decided.
fn every_kind() -> Vec<DiagnosticKind> {
    use crate::marker::{Alias, AnchorId, MalformedReason, MarkerKind, RelPath, SymbolName};
    use crate::scan::SkipReason;

    let rel = |path: &str| RelPath::parse(path).unwrap();
    let id = || AnchorId::parse("auth/flow").unwrap();
    let alias = || Alias::parse("Flow").unwrap();

    vec![
        DiagnosticKind::Unresolved(Unresolved::PathMissing {
            root: root(),
            path: rel("a.md"),
        }),
        DiagnosticKind::Unresolved(Unresolved::PathNotDirectory {
            root: root(),
            path: rel("a.md"),
        }),
        DiagnosticKind::Unresolved(Unresolved::PathNotFile {
            root: root(),
            path: rel("a.md"),
        }),
        DiagnosticKind::Unresolved(Unresolved::PathEscapesRoot {
            root: root(),
            path: rel("a.md"),
        }),
        DiagnosticKind::Unresolved(Unresolved::SymbolMissing {
            root: root(),
            path: rel("a.rs"),
            name: SymbolName::parse("Name").unwrap(),
        }),
        DiagnosticKind::Unresolved(Unresolved::AnchorMissing {
            root: root(),
            id: id(),
        }),
        DiagnosticKind::Unresolved(Unresolved::RootUndeclared { name: root() }),
        DiagnosticKind::DuplicateAnchor {
            root: root(),
            id: id(),
        },
        DiagnosticKind::AliasUndeclared {
            root: root(),
            path: FilePath::new(Utf8PathBuf::from("a.md")).unwrap(),
            alias: alias(),
        },
        DiagnosticKind::AliasDuplicate {
            root: root(),
            path: FilePath::new(Utf8PathBuf::from("a.md")).unwrap(),
            alias: alias(),
        },
        DiagnosticKind::Malformed {
            kind: MarkerKind::Ref,
            reason: MalformedReason::Unclosed,
        },
        DiagnosticKind::Unverified(Unverified::RootAbsent {
            name: root(),
            declared_dir: Utf8PathBuf::from("/nowhere"),
        }),
        DiagnosticKind::Unverified(Unverified::NoGrammar {
            root: root(),
            path: rel("a.ex"),
            extension: Some("ex".to_owned()),
        }),
        DiagnosticKind::Unverified(Unverified::NoGrammar {
            root: root(),
            path: rel("LICENSE"),
            extension: None,
        }),
        DiagnosticKind::Unverified(Unverified::ParseErrors {
            root: root(),
            path: rel("a.rs"),
            language: "rust",
        }),
        DiagnosticKind::Unverified(Unverified::ParseTimeout {
            root: root(),
            path: rel("a.rs"),
        }),
        DiagnosticKind::Unverified(Unverified::SymbolTableTruncated {
            root: root(),
            path: rel("a.rs"),
        }),
        DiagnosticKind::Unverified(Unverified::TargetTooLarge {
            root: root(),
            path: rel("a.rs"),
            bytes: 2,
            limit: 1,
        }),
        DiagnosticKind::Unverified(Unverified::TargetNotUtf8 {
            root: root(),
            path: rel("a.rs"),
        }),
        DiagnosticKind::Unverified(Unverified::TargetUnreadable {
            root: root(),
            path: rel("a.rs"),
            message: "denied".to_owned(),
        }),
        DiagnosticKind::Unverified(Unverified::AnalyzeFailed {
            root: root(),
            path: rel("a.rs"),
            message: "boom".to_owned(),
        }),
        DiagnosticKind::ExternalDuplicate {
            root: root(),
            id: id(),
        },
        DiagnosticKind::FileSkipped {
            root: root(),
            reason: SkipReason::NotUtf8,
        },
        DiagnosticKind::FileSkipped {
            root: root(),
            reason: SkipReason::TooLarge { bytes: 2, limit: 1 },
        },
        DiagnosticKind::WalkProblem {
            root: root(),
            message: "boom".to_owned(),
        },
    ]
}

#[test]
fn every_kind_has_a_non_empty_title_naming_its_subject() {
    for kind in every_kind() {
        let title = kind.to_string();
        assert!(!title.is_empty(), "{kind:?} has no title");
        assert!(
            !title.contains("{") && !title.contains("Unresolved("),
            "{kind:?} rendered a debug shape: {title}"
        );
    }
}

/// No two kinds may read identically, or a reader cannot tell which finding they have.
#[test]
fn no_two_kinds_render_the_same_title() {
    let titles: Vec<String> = every_kind().iter().map(ToString::to_string).collect();
    let distinct: std::collections::HashSet<&String> = titles.iter().collect();
    assert_eq!(
        distinct.len(),
        titles.len(),
        "duplicate title in {titles:#?}"
    );
}

#[test]
fn a_hint_points_at_the_setting_or_edit_that_resolves_the_finding() {
    let hint_for = |kind: &DiagnosticKind| kind.hint().unwrap_or_default();

    let no_grammar = hint_for(&DiagnosticKind::Unverified(Unverified::NoGrammar {
        root: root(),
        path: crate::marker::RelPath::parse("a.ex").unwrap(),
        extension: Some("ex".to_owned()),
    }));
    assert!(no_grammar.contains("`.ex`"), "{no_grammar}");

    let no_extension = hint_for(&DiagnosticKind::Unverified(Unverified::NoGrammar {
        root: root(),
        path: crate::marker::RelPath::parse("LICENSE").unwrap(),
        extension: None,
    }));
    assert!(no_extension.contains("no extension"), "{no_extension}");

    let timeout = hint_for(&DiagnosticKind::Unverified(Unverified::ParseTimeout {
        root: root(),
        path: crate::marker::RelPath::parse("a.rs").unwrap(),
    }));
    assert!(timeout.contains("scan.parse-budget-ms"), "{timeout}");

    let too_large = hint_for(&DiagnosticKind::Unverified(Unverified::TargetTooLarge {
        root: root(),
        path: crate::marker::RelPath::parse("a.rs").unwrap(),
        bytes: 2,
        limit: 1,
    }));
    assert!(too_large.contains("scan.max-file-bytes"), "{too_large}");

    let undeclared = hint_for(&DiagnosticKind::AliasUndeclared {
        root: root(),
        path: FilePath::new(Utf8PathBuf::from("a.md")).unwrap(),
        alias: crate::marker::Alias::parse("Flow").unwrap(),
    });
    assert!(undeclared.contains("@ref[target as Flow]"), "{undeclared}");
}

#[test]
fn a_finding_with_nothing_actionable_offers_no_hint() {
    assert!(
        DiagnosticKind::Unresolved(Unresolved::PathMissing {
            root: root(),
            path: crate::marker::RelPath::parse("a.md").unwrap(),
        })
        .hint()
        .is_none()
    );
}
