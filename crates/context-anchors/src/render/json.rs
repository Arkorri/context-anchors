//! The machine-readable contract. Its own DTOs, decoupled from the core types, versioned by
//! `schema`. Consumers are agents and CI, so codes are stable strings and every location
//! carries both one-based line/column and byte offsets.

use std::io::Write;

use anchr_core::check::locate;
use anchr_core::coverage::{CandidateKind, CoverageReport};
use anchr_core::diagnostic::{
    Diagnostic, DiagnosticKind, LocatedSite, Locations, Report, Severity,
};
use anchr_core::index::Index;
use anchr_core::resolve::{Unresolved, Unverified};
use anchr_core::text::RegionKind;
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 1;

pub fn write(out: &mut impl Write, report: &Report) -> anyhow::Result<()> {
    let json = JsonReport::from(report);
    serde_json::to_writer_pretty(&mut *out, &json)?;
    writeln!(out)?;
    Ok(())
}

/// The `backrefs` report: every site referring to one target.
pub fn write_sites(
    out: &mut impl Write,
    target: &str,
    sites: &[LocatedSite],
) -> anyhow::Result<()> {
    let json = JsonSites {
        schema: SCHEMA_VERSION,
        target,
        sites: sites.iter().map(JsonLocation::from).collect(),
    };
    serde_json::to_writer_pretty(&mut *out, &json)?;
    writeln!(out)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonSites<'a> {
    schema: u32,
    target: &'a str,
    sites: Vec<JsonLocation<'a>>,
}

impl<'a> From<&'a LocatedSite> for JsonLocation<'a> {
    fn from(located: &'a LocatedSite) -> Self {
        JsonLocation {
            root: located.site.root.as_str(),
            path: Some(located.site.path.as_str()),
            line: Some(located.line_col.line),
            col: Some(located.line_col.col),
            byte_start: Some(located.site.span.start),
            byte_end: Some(located.site.span.end),
            region: Some(region_name(located.site.region)),
        }
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    schema: u32,
    summary: JsonSummary,
    roots: Vec<JsonRoot<'a>>,
    diagnostics: Vec<JsonDiagnostic<'a>>,
}

#[derive(Serialize)]
struct JsonSummary {
    roots_scanned: usize,
    files_scanned: usize,
    anchors: usize,
    refs_checked: usize,
    refs_resolved: usize,
    alias_uses: usize,
    errors: usize,
    unverified: usize,
    strict: bool,
}

#[derive(Serialize)]
struct JsonRoot<'a> {
    name: &'a str,
    dir: &'a str,
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    code: &'static str,
    severity: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    notes: &'a [String],
    locations: Vec<JsonLocation<'a>>,
}

#[derive(Serialize)]
struct JsonLocation<'a> {
    root: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    /// One-based byte column.
    #[serde(skip_serializing_if = "Option::is_none")]
    col: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_end: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<&'static str>,
}

impl<'a> From<&'a Report> for JsonReport<'a> {
    fn from(report: &'a Report) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            summary: JsonSummary {
                roots_scanned: report.summary.roots_scanned,
                files_scanned: report.summary.files_scanned,
                anchors: report.summary.anchors,
                refs_checked: report.summary.refs_checked,
                refs_resolved: report.summary.refs_resolved,
                alias_uses: report.summary.alias_uses,
                errors: report.summary.errors,
                unverified: report.summary.unverified,
                strict: report.policy == anchr_core::config::UnverifiedPolicy::Error,
            },
            roots: report
                .root_dirs
                .iter()
                .map(|(name, dir)| JsonRoot {
                    name: name.as_str(),
                    dir: dir.as_str(),
                })
                .collect(),
            diagnostics: report
                .diagnostics
                .iter()
                .map(JsonDiagnostic::from)
                .collect(),
        }
    }
}

impl<'a> From<&'a Diagnostic> for JsonDiagnostic<'a> {
    fn from(diagnostic: &'a Diagnostic) -> Self {
        let locations = match &diagnostic.locations {
            Locations::Sites(sites) => sites.iter().map(JsonLocation::from).collect(),
            Locations::Files(files) => files
                .iter()
                .map(|file| JsonLocation {
                    root: file.root.as_str(),
                    path: Some(file.path.as_str()),
                    line: None,
                    col: None,
                    byte_start: None,
                    byte_end: None,
                    region: None,
                })
                .collect(),
            Locations::Roots(roots) => roots
                .iter()
                .map(|root| JsonLocation {
                    root: root.as_str(),
                    path: None,
                    line: None,
                    col: None,
                    byte_start: None,
                    byte_end: None,
                    region: None,
                })
                .collect(),
        };
        Self {
            code: code(&diagnostic.kind),
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Unverified => "unverified",
            },
            message: diagnostic.kind.to_string(),
            suggestion: diagnostic.suggestion.as_deref(),
            hint: diagnostic.kind.hint(),
            notes: &diagnostic.notes,
            locations,
        }
    }
}

fn region_name(region: RegionKind) -> &'static str {
    match region {
        RegionKind::Prose => "prose",
        RegionKind::Comment => "comment",
        RegionKind::Whole => "plaintext",
        RegionKind::InlineCode => "inline-code",
    }
}

/// The `coverage` report.
pub fn write_coverage(
    out: &mut impl Write,
    index: &Index,
    report: &CoverageReport,
) -> anyhow::Result<()> {
    let mut candidates = Vec::with_capacity(report.candidates.len());
    for candidate in &report.candidates {
        let located = locate(index, candidate.site.clone())?;
        let (kind, replacement, reason, declared_in) = match &candidate.kind {
            CandidateKind::Proposal { replacement } => {
                ("proposal", Some(replacement.clone()), None, Vec::new())
            }
            CandidateKind::Unresolvable { reason } => {
                ("unresolvable", None, Some(reason.clone()), Vec::new())
            }
            CandidateKind::Ambiguous { declared_in } => (
                "ambiguous",
                None,
                None,
                declared_in.iter().map(ToString::to_string).collect(),
            ),
            CandidateKind::UnusedAlias { .. } => ("unused-alias", None, None, Vec::new()),
        };
        candidates.push(JsonCandidate {
            kind,
            text: candidate.text.clone(),
            replacement,
            reason,
            declared_in,
            location: JsonOwnedLocation {
                root: located.site.root.to_string(),
                path: located.site.path.to_string(),
                line: located.line_col.line,
                col: located.line_col.col,
                byte_start: located.site.span.start,
                byte_end: located.site.span.end,
                region: region_name(located.site.region),
            },
        });
    }
    let json = JsonCoverage {
        schema: SCHEMA_VERSION,
        summary: JsonCoverageSummary {
            annotated_refs: report.summary.annotated_refs,
            total: report.summary.total(),
            proposals: report.summary.proposals,
            unresolvable: report.summary.unresolvable,
            ambiguous: report.summary.ambiguous,
            unused_aliases: report.summary.unused_aliases,
        },
        candidates,
    };
    serde_json::to_writer_pretty(&mut *out, &json)?;
    writeln!(out)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonCoverage {
    schema: u32,
    summary: JsonCoverageSummary,
    candidates: Vec<JsonCandidate>,
}

#[derive(Serialize)]
struct JsonCoverageSummary {
    annotated_refs: usize,
    total: usize,
    proposals: usize,
    unresolvable: usize,
    ambiguous: usize,
    unused_aliases: usize,
}

#[derive(Serialize)]
struct JsonCandidate {
    kind: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    declared_in: Vec<String>,
    location: JsonOwnedLocation,
}

#[derive(Serialize)]
struct JsonOwnedLocation {
    root: String,
    path: String,
    line: u32,
    col: u32,
    byte_start: usize,
    byte_end: usize,
    region: &'static str,
}

/// Stable identifiers for consumers; adding a kind adds a code, never renames one.
pub fn code(kind: &DiagnosticKind) -> &'static str {
    match kind {
        DiagnosticKind::Unresolved(unresolved) => match unresolved {
            Unresolved::PathMissing { .. } => "path-missing",
            Unresolved::PathNotDirectory { .. } => "path-not-directory",
            Unresolved::PathNotFile { .. } => "path-not-file",
            Unresolved::PathEscapesRoot { .. } => "path-escapes-root",
            Unresolved::SymbolMissing { .. } => "symbol-missing",
            Unresolved::AnchorMissing { .. } => "anchor-missing",
            Unresolved::RootUndeclared { .. } => "root-undeclared",
        },
        DiagnosticKind::DuplicateAnchor { .. } => "duplicate-anchor",
        DiagnosticKind::AliasUndeclared { .. } => "alias-undeclared",
        DiagnosticKind::AliasDuplicate { .. } => "alias-duplicate",
        DiagnosticKind::Malformed { .. } => "malformed-marker",
        DiagnosticKind::Unverified(unverified) => match unverified {
            Unverified::RootAbsent { .. } => "root-absent",
            Unverified::NoGrammar { .. } => "no-grammar",
            Unverified::ParseErrors { .. } => "parse-errors",
            Unverified::ParseTimeout { .. } => "parse-timeout",
            Unverified::SymbolTableTruncated { .. } => "symbol-table-truncated",
            Unverified::TargetTooLarge { .. } => "target-too-large",
            Unverified::TargetNotUtf8 { .. } => "target-not-utf8",
            Unverified::TargetUnreadable { .. } => "target-unreadable",
            Unverified::AnalyzeFailed { .. } => "analyze-failed",
        },
        DiagnosticKind::ExternalDuplicate { .. } => "external-duplicate",
        DiagnosticKind::FileSkipped { .. } => "file-skipped",
        DiagnosticKind::WalkProblem { .. } => "walk-problem",
    }
}
