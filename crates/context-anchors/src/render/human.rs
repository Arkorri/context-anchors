//! rustc-shaped output: one titled group per cause, the first site as a source snippet, the
//! remaining sites as `--> path:line:col` origins, then the suggestion or hint.

use std::io::Write;

use anchr_core::diagnostic::{Diagnostic, Locations, Report, Severity};
use annotate_snippets::{AnnotationKind, Group, Level, Origin, Renderer, Snippet};

/// Sites beyond this many are summarized as a count.
const MAX_LISTED_SITES: usize = 40;

pub fn write(out: &mut impl Write, report: &Report) -> std::io::Result<()> {
    let renderer = Renderer::styled();
    for diagnostic in &report.diagnostics {
        let source = first_site_source(report, diagnostic);
        let group = group_for(diagnostic, source.as_deref());
        writeln!(out, "{}\n", renderer.render(&[group]))?;
    }
    write_summary(out, report)
}

fn group_for<'a>(diagnostic: &'a Diagnostic, source: Option<&'a str>) -> Group<'a> {
    let level = match diagnostic.severity {
        Severity::Error => Level::ERROR,
        Severity::Unverified => Level::WARNING.with_name("unverified"),
    };
    let title = diagnostic.kind.to_string();
    let mut group = Group::with_title(level.primary_title(title));

    match &diagnostic.locations {
        Locations::Sites(sites) => {
            let mut remaining = sites.iter();
            if let (Some(first), Some(source)) = (sites.first(), source) {
                let span = first.site.span.start..first.site.span.end;
                if source.get(span.clone()).is_some() {
                    group = group.element(
                        Snippet::source(source)
                            .path(first.site.path.as_str())
                            .line_start(1)
                            .fold(true)
                            .annotation(AnnotationKind::Primary.span(span)),
                    );
                    remaining.next();
                }
            }
            let listed = remaining.clone().take(MAX_LISTED_SITES);
            for site in listed {
                group = group.element(
                    Origin::path(site.site.path.as_str())
                        .line(site.line_col.line as usize)
                        .char_column(site.line_col.col as usize),
                );
            }
            let unlisted = remaining.count().saturating_sub(MAX_LISTED_SITES);
            if unlisted > 0 {
                group =
                    group.element(Level::NOTE.message(format!("and {unlisted} more locations")));
            }
        }
        Locations::Files(files) => {
            for file in files {
                group = group.element(Origin::path(file.path.as_str()));
            }
        }
        Locations::Roots(roots) => {
            for root in roots {
                group = group.element(Level::NOTE.message(format!("in root `{root}`")));
            }
        }
    }

    if let Some(suggestion) = &diagnostic.suggestion {
        group = group.element(Level::HELP.message(format!("did you mean `{suggestion}`?")));
    }
    if let Some(hint) = diagnostic.kind.hint() {
        group = group.element(Level::HELP.message(hint));
    }
    group
}

/// The source of the file holding the first site, if it can still be read. A file that
/// changed or vanished between scan and render degrades to the origin list.
fn first_site_source(report: &Report, diagnostic: &Diagnostic) -> Option<String> {
    let Locations::Sites(sites) = &diagnostic.locations else {
        return None;
    };
    let first = sites.first()?;
    let dir = report.root_dirs.get(&first.site.root)?;
    std::fs::read_to_string(dir.join(first.site.path.as_path())).ok()
}

fn write_summary(out: &mut impl Write, report: &Report) -> std::io::Result<()> {
    let summary = report.summary;
    writeln!(
        out,
        "checked {} references in {} files ({} anchors): {} resolved, {} errors, {} unverified",
        summary.refs_checked,
        summary.files_scanned,
        summary.anchors,
        summary.refs_resolved,
        summary.errors,
        summary.unverified,
    )
}
