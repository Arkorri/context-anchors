use std::io::Write;

use anchr_core::check::Workspace;
use anchr_core::coverage::coverage;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod coverage_tests;

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
    write_report(&mut stdout, index, &report, args.format)?;
    stdout.flush()?;
    Ok(Outcome::Clean)
}

fn write_report(
    out: &mut impl Write,
    index: &anchr_core::index::Index,
    report: &anchr_core::coverage::CoverageReport,
    format: Format,
) -> anyhow::Result<()> {
    match format {
        Format::Json => render::json::write_coverage(out, index, report),
        Format::Human => render::coverage::write(out, index, report),
    }
}
