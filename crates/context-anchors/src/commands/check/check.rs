use std::ffi::OsString;
use std::io::Write;

use anchr_core::check::{CheckOptions, run_check};
use anchr_core::config::UnverifiedPolicy;
use camino::{Utf8Path, Utf8PathBuf};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod check_tests;

use super::{Outcome, current_dir, discover, files_in_root};
use crate::cli::{CheckArgs, CheckFormat};
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
        CheckFormat::Human => render::human::write(&mut stdout, &report)?,
        CheckFormat::Json => render::json::write(&mut stdout, &report)?,
        CheckFormat::Github => {
            let workspace = workspace_root(&cwd, std::env::var_os("GITHUB_WORKSPACE"));
            render::github::write(&mut stdout, &report, &workspace)?;
        }
    }
    stdout.flush()?;

    Ok(outcome_for(&report))
}

/// The directory GitHub annotations are relative to: the checkout named by `GITHUB_WORKSPACE`
/// when the runner sets it, since a step's `working-directory` may be a subdirectory of it, and
/// the current directory otherwise. The variable is passed in so tests never touch process state.
fn workspace_root(cwd: &Utf8Path, github_workspace: Option<OsString>) -> Utf8PathBuf {
    github_workspace
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .map_or_else(|| cwd.to_path_buf(), Utf8PathBuf::from)
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
