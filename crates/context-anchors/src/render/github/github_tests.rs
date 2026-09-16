use std::collections::BTreeMap;

use anchr_core::config::UnverifiedPolicy;
use anchr_core::diagnostic::{DiagnosticKind, FileLocation, LocatedSite, Summary};
use anchr_core::index::Site;
use anchr_core::marker::RelPath;
use anchr_core::resolve::Unresolved;
use anchr_core::root::{FilePath, RootName};
use anchr_core::span::{ByteSpan, LineCol};
use anchr_core::text::RegionKind;
use camino::Utf8PathBuf;

use super::*;
use crate::render::MAX_LISTED_SITES;

fn root() -> RootName {
    RootName::parse("r").unwrap()
}

fn file_path(path: &str) -> FilePath {
    FilePath::new(Utf8PathBuf::from(path)).unwrap()
}

fn site(path: &str, line: u32, col: u32) -> LocatedSite {
    LocatedSite {
        site: Site {
            root: root(),
            path: file_path(path),
            span: ByteSpan::new(0, 4),
            region: RegionKind::Prose,
        },
        line_col: LineCol { line, col },
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

/// The root sits at `/w/pkg`, a directory that does not exist, so the human part never reads
/// source and the test stays off the filesystem.
fn report(diagnostics: Vec<Diagnostic>) -> Report {
    let mut root_dirs = BTreeMap::new();
    root_dirs.insert(root(), Utf8PathBuf::from("/w/pkg"));
    Report {
        diagnostics,
        summary: Summary::default(),
        policy: UnverifiedPolicy::Report,
        root_dirs,
    }
}

fn rendered(report: &Report, workspace: &str) -> String {
    let mut out = Vec::new();
    write_with(
        &mut out,
        &Renderer::plain(),
        report,
        Utf8Path::new(workspace),
    )
    .unwrap();
    String::from_utf8(out).unwrap()
}

fn annotations(report: &Report, workspace: &str) -> Vec<String> {
    rendered(report, workspace)
        .lines()
        .filter(|line| line.starts_with("::"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn errors_become_error_commands_and_unverified_become_warnings() {
    let lines = annotations(
        &report(vec![
            diagnostic(Severity::Error, Locations::Sites(vec![site("a.md", 3, 5)])),
            diagnostic(
                Severity::Unverified,
                Locations::Sites(vec![site("b.md", 1, 1)]),
            ),
        ]),
        "/w",
    );
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("::error "), "{}", lines[0]);
    assert!(lines[1].starts_with("::warning "), "{}", lines[1]);
}

#[test]
fn the_human_report_precedes_the_annotations() {
    let text = rendered(
        &report(vec![diagnostic(
            Severity::Error,
            Locations::Sites(vec![site("a.md", 3, 5)]),
        )]),
        "/w",
    );
    let summary = text.find("checked 0 references").unwrap();
    let annotation = text.find("::error").unwrap();
    assert!(summary < annotation, "{text}");
}

/// The human cap is a reading aid; the annotation list is the record.
#[test]
fn every_site_gets_its_own_line_beyond_the_human_cap() {
    let sites: Vec<LocatedSite> = (0..MAX_LISTED_SITES + 2)
        .map(|n| site("a.md", u32::try_from(n + 1).unwrap(), 1))
        .collect();
    let lines = annotations(
        &report(vec![diagnostic(Severity::Error, Locations::Sites(sites))]),
        "/w",
    );
    assert_eq!(lines.len(), MAX_LISTED_SITES + 2);
}

#[test]
fn the_file_is_relative_to_the_workspace_when_the_root_is_inside_it() {
    let lines = annotations(
        &report(vec![diagnostic(
            Severity::Error,
            Locations::Sites(vec![site("docs/a.md", 3, 5)]),
        )]),
        "/w",
    );
    assert_eq!(
        lines[0],
        "::error file=pkg/docs/a.md,line=3,col=5,title=path-missing::missing path `gone.md` in root `r`"
    );
}

#[test]
fn a_root_at_the_workspace_uses_the_bare_site_path() {
    let lines = annotations(
        &report(vec![diagnostic(
            Severity::Error,
            Locations::Sites(vec![site("docs/a.md", 3, 5)]),
        )]),
        "/w/pkg",
    );
    assert!(
        lines[0].starts_with("::error file=docs/a.md,"),
        "{}",
        lines[0]
    );
}

#[test]
fn a_root_outside_the_workspace_moves_the_location_into_the_message() {
    let lines = annotations(
        &report(vec![diagnostic(
            Severity::Error,
            Locations::Sites(vec![site("docs/a.md", 3, 5)]),
        )]),
        "/elsewhere",
    );
    assert_eq!(
        lines[0],
        "::error title=path-missing::r:docs/a.md:3:5: missing path `gone.md` in root `r`"
    );
}

#[test]
fn an_absent_root_directory_counts_as_outside_the_workspace() {
    let mut report = report(vec![diagnostic(
        Severity::Error,
        Locations::Sites(vec![site("docs/a.md", 3, 5)]),
    )]);
    report.root_dirs.clear();
    assert!(!annotations(&report, "/w")[0].contains("file="));
}

#[test]
fn file_locations_carry_no_line_and_root_locations_no_file() {
    let lines = annotations(
        &report(vec![
            diagnostic(
                Severity::Unverified,
                Locations::Files(vec![FileLocation {
                    root: root(),
                    path: file_path("big.md"),
                }]),
            ),
            diagnostic(Severity::Unverified, Locations::Roots(vec![root()])),
        ]),
        "/w",
    );
    assert_eq!(
        lines[0],
        "::warning file=pkg/big.md,title=path-missing::missing path `gone.md` in root `r`"
    );
    assert_eq!(
        lines[1],
        "::warning title=path-missing::missing path `gone.md` in root `r` (root `r`)"
    );
}

#[test]
fn a_suggestion_is_appended_to_the_message() {
    let mut diagnostic = diagnostic(Severity::Error, Locations::Sites(vec![site("a.md", 1, 1)]));
    diagnostic.suggestion = Some("gone.txt".to_owned());
    let lines = annotations(&report(vec![diagnostic]), "/w");
    assert!(
        lines[0].ends_with("; did you mean `gone.txt`?"),
        "{}",
        lines[0]
    );
}

#[test]
fn the_title_is_the_json_code() {
    let lines = annotations(
        &report(vec![diagnostic(
            Severity::Error,
            Locations::Sites(vec![site("a.md", 1, 1)]),
        )]),
        "/w",
    );
    assert!(
        lines[0].contains(&format!("title={}", json::code(&missing_path()))),
        "{}",
        lines[0]
    );
}

#[test]
fn percent_is_escaped_first_so_a_literal_code_survives() {
    assert_eq!(escape_data("100%0A"), "100%250A");
    assert_eq!(escape_data("a\nb\rc"), "a%0Ab%0Dc");
}

#[test]
fn property_values_escape_comma_and_colon_but_messages_do_not() {
    assert_eq!(escape_property("a,b:c%"), "a%2Cb%3Ac%25");
    assert_eq!(escape_data("a,b:c"), "a,b:c");
}

#[test]
fn a_command_without_properties_has_no_space_after_the_level() {
    assert_eq!(command("error", &[], "m"), "::error::m");
    assert_eq!(
        command("error", &["file=a".to_owned(), "line=1".to_owned()], "m"),
        "::error file=a,line=1::m"
    );
}

#[test]
fn newlines_in_a_message_never_reach_the_log_line() {
    let text = command("error", &[], "first\nsecond");
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("first%0Asecond"));
}
