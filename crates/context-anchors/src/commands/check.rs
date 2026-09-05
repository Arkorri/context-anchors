use std::io::Write;

use anchr_core::check::{CheckOptions, run_check};
use anchr_core::config::UnverifiedPolicy;

use super::{Outcome, current_dir, discover, files_in_root};
use crate::cli::{CheckArgs, Format};
use crate::render;

pub fn run(args: &CheckArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let discovered = discover(&cwd, args.root.as_deref())?;
    let only_files = files_in_root(&cwd, &discovered.root_dir, &args.paths)?;

    let options = CheckOptions {
        unverified: args.strict.then_some(UnverifiedPolicy::Error),
        only_files,
    };
    let report = run_check(discovered, &options)?;

    let mut stdout = anstream::stdout().lock();
    match args.format {
        Format::Human => render::human::write(&mut stdout, &report)?,
        Format::Json => render::json::write(&mut stdout, &report)?,
    }
    stdout.flush()?;

    Ok(if report.has_errors() {
        Outcome::Errors
    } else {
        Outcome::Clean
    })
}
