use std::collections::BTreeMap;

use anchr_core::config::UnverifiedPolicy;
use anchr_core::diagnostic::{DiagnosticKind, LocatedSite, Summary};
use anchr_core::index::Site;
use anchr_core::marker::RelPath;
use anchr_core::resolve::Unresolved;
use anchr_core::root::{FilePath, RootName};
use anchr_core::span::{ByteSpan, LineCol};
use anchr_core::text::RegionKind;
use camino::Utf8PathBuf;

use super::*;

fn root() -> RootName {
    RootName::parse("r").unwrap()
}

fn site(path: &str, line: u32) -> LocatedSite {
    LocatedSite {
        site: Site {
            root: root(),
            path: FilePath::new(Utf8PathBuf::from(path)).unwrap(),
            span: ByteSpan::new(0, 4),
            region: RegionKind::Prose,
        },
        line_col: LineCol { line, col: 1 },
    }
}

fn missing_path() -> DiagnosticKind {
    DiagnosticKind::Unresolved(Unresolved::PathMissing {
        root: root(),
        path: RelPath::parse("gone.md").unwrap(),
    })
}

fn diagnostic(severity: Severity, locations: Locations) -> Diagnostic {
    Diagnostic {
        kind: missing_path(),
        severity,
        locations,
        suggestion: None,
        notes: Vec::new(),
    }
}

/// No `root_dirs` entry, so `first_site_source` finds nothing and every site renders as an
/// origin. That keeps these tests off the filesystem.
fn report(diagnostics: Vec<Diagnostic>) -> Report {
    Report {
        diagnostics,
        summary: Summary::default(),
        policy: UnverifiedPolicy::Report,
        root_dirs: BTreeMap::new(),
    }
}

fn rendered(report: &Report) -> String {
    let mut out = Vec::new();
    write_with(&mut out, &Renderer::plain(), report).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn an_unverified_finding_is_labelled_rather_than_called_an_error() {
    let errors = rendered(&report(vec![diagnostic(
        Severity::Error,
        Locations::Sites(vec![site("a.md", 3)]),
    )]));
    assert!(errors.contains("error"), "{errors}");

    let unverified = rendered(&report(vec![diagnostic(
        Severity::Unverified,
        Locations::Sites(vec![site("a.md", 3)]),
    )]));
    assert!(unverified.contains("unverified"), "{unverified}");
}

#[test]
fn each_site_is_listed_as_an_origin_when_its_source_cannot_be_read() {
    let text = rendered(&report(vec![diagnostic(
        Severity::Error,
        Locations::Sites(vec![site("a.md", 3), site("b.md", 7)]),
    )]));
    assert!(text.contains("a.md:3:1"), "{text}");
    assert!(text.contains("b.md:7:1"), "{text}");
}

/// Without a snippet the cap applies to the whole list, so the note appears only past
/// `MAX_LISTED_SITES` and counts the remainder.
#[test]
fn sites_past_the_cap_collapse_into_a_counted_note() {
    let at_cap: Vec<LocatedSite> = (0..MAX_LISTED_SITES)
        .map(|n| site("a.md", u32::try_from(n + 1).unwrap()))
        .collect();
    let text = rendered(&report(vec![diagnostic(
        Severity::Error,
        Locations::Sites(at_cap.clone()),
    )]));
    assert!(
        !text.contains("more locations"),
        "no note at exactly the cap: {text}"
    );

    let mut over_cap = at_cap;
    over_cap.push(site("b.md", 99));
    over_cap.push(site("c.md", 100));
    let text = rendered(&report(vec![diagnostic(
        Severity::Error,
        Locations::Sites(over_cap),
    )]));
    assert!(text.contains("and 2 more locations"), "{text}");
}

#[test]
fn root_locations_render_as_notes_naming_the_root() {
    let text = rendered(&report(vec![diagnostic(
        Severity::Error,
        Locations::Roots(vec![root()]),
    )]));
    assert!(text.contains("in root `r`"), "{text}");
}

#[test]
fn a_suggestion_is_phrased_as_a_question() {
    let mut diagnostic = diagnostic(Severity::Error, Locations::Sites(vec![site("a.md", 1)]));
    diagnostic.suggestion = Some("auth/token-refresh".to_owned());
    let text = rendered(&report(vec![diagnostic]));
    assert!(
        text.contains("did you mean `auth/token-refresh`?"),
        "{text}"
    );
}

#[test]
fn notes_reach_the_output_in_the_order_they_were_recorded() {
    let mut diagnostic = diagnostic(Severity::Error, Locations::Sites(vec![site("a.md", 1)]));
    diagnostic.notes = vec!["first note".to_owned(), "second note".to_owned()];
    let text = rendered(&report(vec![diagnostic]));
    let first = text.find("first note").unwrap();
    let second = text.find("second note").unwrap();
    assert!(first < second, "{text}");
}

#[test]
fn the_summary_omits_the_alias_clause_when_no_aliases_were_used() {
    let mut clean = report(Vec::new());
    clean.summary = Summary {
        refs_checked: 2,
        files_scanned: 1,
        refs_resolved: 2,
        ..Default::default()
    };
    let text = rendered(&clean);
    assert!(!text.contains("alias uses"), "{text}");
    assert!(text.contains("checked 2 references in 1 files"), "{text}");

    clean.summary.alias_uses = 5;
    assert!(rendered(&clean).contains("and 5 alias uses"));
}
