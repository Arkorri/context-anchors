use std::io::Write;

use anchr_core::check::{CheckOptions, run_check};
use anchr_core::config::UnverifiedPolicy;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

use super::{Outcome, current_dir, discover, files_in_root};
use crate::cli::{CheckArgs, Format};
use crate::render;

pub fn run(args: &CheckArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let discovered = discover(&cwd, args.root.as_deref())?;
    let only_files = files_in_root(&cwd, &discovered.root_dir, &args.paths)?;

    let options = options_from(args, only_files);
    let report = run_check(discovered, &options)?;

    let mut stdout = anstream::stdout().lock();
    match args.format {
        Format::Human => render::human::write(&mut stdout, &report)?,
        Format::Json => render::json::write(&mut stdout, &report)?,
    }
    stdout.flush()?;

    Ok(outcome_for(&report))
}

fn options_from(args: &CheckArgs, only_files: Vec<anchr_core::root::FilePath>) -> CheckOptions {
    CheckOptions {
        unverified: args.strict.then_some(UnverifiedPolicy::Error),
        only_files,
    }
}

/// Errors set exit 1; anything else, unverified findings included, is a clean run.
fn outcome_for(report: &anchr_core::diagnostic::Report) -> Outcome {
    if report.has_errors() {
        Outcome::Errors
    } else {
        Outcome::Clean
    }
}
