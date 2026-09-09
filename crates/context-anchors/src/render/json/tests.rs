use std::collections::HashSet;

use anchr_core::marker::{Alias, AnchorId, MalformedReason, MarkerKind, RelPath, SymbolName};
use anchr_core::root::{FilePath, RootName};
use anchr_core::scan::SkipReason;
use camino::Utf8PathBuf;

use super::*;

fn root() -> RootName {
    RootName::parse("r").unwrap()
}

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap()
}

fn file(path: &str) -> FilePath {
    FilePath::new(Utf8PathBuf::from(path)).unwrap()
}

/// Every diagnostic kind, so a new variant fails to compile here until it is given a code.
fn every_kind() -> Vec<DiagnosticKind> {
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
            id: AnchorId::parse("x").unwrap(),
        }),
        DiagnosticKind::Unresolved(Unresolved::RootUndeclared { name: root() }),
        DiagnosticKind::DuplicateAnchor {
            root: root(),
            id: AnchorId::parse("x").unwrap(),
        },
        DiagnosticKind::AliasUndeclared {
            root: root(),
            path: file("a.md"),
            alias: Alias::parse("Flow").unwrap(),
        },
        DiagnosticKind::AliasDuplicate {
            root: root(),
            path: file("a.md"),
            alias: Alias::parse("Flow").unwrap(),
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
            id: AnchorId::parse("x").unwrap(),
        },
        DiagnosticKind::FileSkipped {
            root: root(),
            reason: SkipReason::NotUtf8,
        },
        DiagnosticKind::WalkProblem {
            root: root(),
            message: "boom".to_owned(),
        },
    ]
}

#[test]
fn every_kind_maps_to_its_documented_code() {
    let expected = [
        "path-missing",
        "path-not-directory",
        "path-not-file",
        "path-escapes-root",
        "symbol-missing",
        "anchor-missing",
        "root-undeclared",
        "duplicate-anchor",
        "alias-undeclared",
        "alias-duplicate",
        "malformed-marker",
        "root-absent",
        "no-grammar",
        "parse-errors",
        "parse-timeout",
        "symbol-table-truncated",
        "target-too-large",
        "target-not-utf8",
        "target-unreadable",
        "analyze-failed",
        "external-duplicate",
        "file-skipped",
        "walk-problem",
    ];
    let actual: Vec<&str> = every_kind().iter().map(code).collect();
    assert_eq!(actual, expected);
}

/// Codes are a stable contract: adding a kind adds a code, and never reuses one.
#[test]
fn no_two_kinds_share_a_code() {
    let codes: Vec<&str> = every_kind().iter().map(code).collect();
    let distinct: HashSet<&&str> = codes.iter().collect();
    assert_eq!(distinct.len(), codes.len(), "duplicate code in {codes:?}");
}

#[test]
fn the_backrefs_report_carries_the_schema_and_an_empty_site_list() {
    let mut out = Vec::new();
    write_sites(&mut out, "#auth/flow", &[]).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.ends_with('\n'),
        "output is newline-terminated: {text:?}"
    );

    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema"], SCHEMA_VERSION);
    assert_eq!(value["target"], "#auth/flow");
    assert_eq!(value["sites"].as_array().unwrap().len(), 0);
}
