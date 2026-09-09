use anchr_core::coverage::CoverageReport;
use anchr_core::index::Index;
use anchr_core::marker::{Alias, NoRefEntry};
use anchr_core::root::RootName;

use super::*;

fn rendered(summary: CoverageSummary) -> String {
    let mut out = Vec::new();
    write_summary(&mut out, summary).unwrap();
    String::from_utf8(out).unwrap()
}

fn empty_index() -> Index {
    Index::new(RootName::parse("r").unwrap())
}

#[test]
fn every_verdict_names_its_kind() {
    assert_eq!(
        verdict(&CandidateKind::Proposal {
            replacement: "@ref[a.md]".to_owned()
        }),
        "could be @ref[a.md]"
    );
    assert_eq!(
        verdict(&CandidateKind::Unresolvable {
            reason: "no such file".to_owned()
        }),
        "does not resolve: no such file"
    );
    assert_eq!(
        verdict(&CandidateKind::UnusedAlias {
            alias: Alias::parse("Flow").unwrap()
        }),
        "alias declared but never used"
    );
    assert_eq!(
        verdict(&CandidateKind::UnusedIgnore {
            entry: NoRefEntry::parse("src/**").unwrap()
        }),
        "ignored but never matched"
    );
}

#[test]
fn a_summary_with_nothing_advisory_carries_no_trailing_clauses() {
    let summary = CoverageSummary {
        annotated_refs: 3,
        proposals: 1,
        unresolvable: 1,
        ..Default::default()
    };
    assert_eq!(
        rendered(summary),
        "3 of 5 reference-shaped strings are annotated; 1 could be, 1 do not resolve\n"
    );
}

#[test]
fn each_advisory_clause_is_singular_at_one_and_plural_above() {
    let one = CoverageSummary {
        unused_aliases: 1,
        ignored: 1,
        unused_ignores: 1,
        ..Default::default()
    };
    let line = rendered(one);
    assert!(
        line.contains("; 1 alias is declared but never used"),
        "{line}"
    );
    assert!(line.contains("; 1 string ignored"), "{line}");
    assert!(line.contains("; 1 ignore entry never matched"), "{line}");

    let many = CoverageSummary {
        unused_aliases: 2,
        ignored: 3,
        unused_ignores: 4,
        ..Default::default()
    };
    let line = rendered(many);
    assert!(
        line.contains("; 2 aliases are declared but never used"),
        "{line}"
    );
    assert!(line.contains("; 3 strings ignored"), "{line}");
    assert!(line.contains("; 4 ignore entries never matched"), "{line}");
}

#[test]
fn advisory_counts_stay_out_of_the_total() {
    let summary = CoverageSummary {
        annotated_refs: 2,
        proposals: 1,
        unresolvable: 0,
        unused_aliases: 9,
        ignored: 9,
        unused_ignores: 9,
    };
    assert_eq!(summary.total(), 3);
    assert!(rendered(summary).starts_with("2 of 3 reference-shaped strings are annotated"));
}

#[test]
fn a_clean_report_renders_the_summary_alone_with_no_separator() {
    let report = CoverageReport {
        candidates: Vec::new(),
        unused_config_ignores: Vec::new(),
        summary: CoverageSummary {
            annotated_refs: 4,
            ..Default::default()
        },
    };
    let mut out = Vec::new();
    write(&mut out, &empty_index(), &report).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(
        text.lines().count(),
        1,
        "a clean report is one line: {text:?}"
    );
    assert!(
        text.starts_with("4 of 4 reference-shaped strings are annotated"),
        "{text}"
    );
}

#[test]
fn an_unused_config_ignore_is_listed_against_the_config_file() {
    let report = CoverageReport {
        candidates: Vec::new(),
        unused_config_ignores: vec![NoRefEntry::parse("src/**").unwrap()],
        summary: CoverageSummary {
            unused_ignores: 1,
            ..Default::default()
        },
    };
    let mut out = Vec::new();
    write(&mut out, &empty_index(), &report).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("`src/**` — ignored but never matched"),
        "{text}"
    );
    assert!(text.contains(&format!(" --> {CONFIG_FILE_NAME}")), "{text}");
}
