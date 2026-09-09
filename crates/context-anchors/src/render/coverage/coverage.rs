//! The `coverage` report for humans: one titled group per token and verdict, its sites as
//! `--> path:line:col` lines capped like `check`'s, then the summary. No source snippets:
//! coverage is advisory, and a snippet per group would bury the report it exists to shorten.

use std::io::Write;

use anchr_core::check::locate;
use anchr_core::config::CONFIG_FILE_NAME;
use anchr_core::coverage::{CandidateKind, CoverageReport, CoverageSummary};
use anchr_core::index::Index;

use super::MAX_LISTED_SITES;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod coverage_tests;

pub fn write(out: &mut impl Write, index: &Index, report: &CoverageReport) -> anyhow::Result<()> {
    for group in &report.candidates {
        writeln!(out, "`{}` — {}", group.token, verdict(&group.kind))?;
        for candidate in group.sites.iter().take(MAX_LISTED_SITES) {
            let located = locate(index, candidate.site.clone())?;
            writeln!(out, " --> {}:{}", located.site.path, located.line_col)?;
        }
        let unlisted = group.sites.len().saturating_sub(MAX_LISTED_SITES);
        if unlisted > 0 {
            writeln!(out, " = note: and {unlisted} more sites")?;
        }
    }
    for entry in &report.unused_config_ignores {
        writeln!(out, "`{entry}` — ignored but never matched")?;
        writeln!(out, " --> {CONFIG_FILE_NAME}")?;
    }
    if !report.candidates.is_empty() || !report.unused_config_ignores.is_empty() {
        writeln!(out)?;
    }
    write_summary(out, report.summary)
}

fn verdict(kind: &CandidateKind) -> String {
    match kind {
        CandidateKind::Proposal { replacement } => format!("could be {replacement}"),
        CandidateKind::Unresolvable { reason } => format!("does not resolve: {reason}"),
        CandidateKind::UnusedAlias { .. } => "alias declared but never used".to_owned(),
        CandidateKind::UnusedIgnore { .. } => "ignored but never matched".to_owned(),
    }
}

fn write_summary(out: &mut impl Write, summary: CoverageSummary) -> anyhow::Result<()> {
    let unused_aliases = match summary.unused_aliases {
        0 => String::new(),
        1 => "; 1 alias is declared but never used".to_owned(),
        n => format!("; {n} aliases are declared but never used"),
    };
    let ignored = match summary.ignored {
        0 => String::new(),
        1 => "; 1 string ignored".to_owned(),
        n => format!("; {n} strings ignored"),
    };
    let unused_ignores = match summary.unused_ignores {
        0 => String::new(),
        1 => "; 1 ignore entry never matched".to_owned(),
        n => format!("; {n} ignore entries never matched"),
    };
    writeln!(
        out,
        "{} of {} reference-shaped strings are annotated; {} could be, {} do not resolve{unused_aliases}{ignored}{unused_ignores}",
        summary.annotated_refs,
        summary.total(),
        summary.proposals,
        summary.unresolvable,
    )?;
    Ok(())
}
