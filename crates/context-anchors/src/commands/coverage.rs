use std::io::Write;

use anchr_core::check::Workspace;
use anchr_core::coverage::coverage;

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
        Format::Human => render::coverage::write(&mut stdout, index, &report)?,
    }
    stdout.flush()?;
    Ok(Outcome::Clean)
}
