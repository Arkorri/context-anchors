use std::io::Write;

use anchr_core::check::{Workspace, locate};
use anchr_core::coverage::{CandidateKind, coverage};

use super::{Outcome, current_dir, discover, files_in_root};
use crate::cli::{CoverageArgs, Format};
use crate::render;

pub fn run(args: &CoverageArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let discovered = discover(&cwd, args.root.as_deref())?;
    let only_files = files_in_root(&cwd, &discovered.root_dir, &args.paths)?;
    let workspace = Workspace::load(discovered)?;
    let report = coverage(&workspace, &only_files);
    let (_, index) = workspace.current();

    let mut stdout = anstream::stdout().lock();
    match args.format {
        Format::Json => render::json::write_coverage(&mut stdout, index, &report)?,
        Format::Human => {
            for candidate in &report.candidates {
                let located = locate(index, candidate.site.clone())?;
                let verdict = match &candidate.kind {
                    CandidateKind::Proposal { replacement } => format!("could be {replacement}"),
                    CandidateKind::Unresolvable { reason } => format!("does not resolve: {reason}"),
                    CandidateKind::Ambiguous { declared_in } => format!(
                        "declared in {} files: {}",
                        declared_in.len(),
                        declared_in
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                writeln!(
                    stdout,
                    "{}:{}: {} — {verdict}",
                    located.site.path, located.line_col, candidate.text
                )?;
            }
            let summary = report.summary;
            writeln!(
                stdout,
                "{} of {} reference-shaped strings are annotated; {} could be, {} do not resolve, {} are ambiguous",
                summary.annotated_refs,
                summary.total(),
                summary.proposals,
                summary.unresolvable,
                summary.ambiguous,
            )?;
        }
    }
    stdout.flush()?;
    Ok(Outcome::Clean)
}
