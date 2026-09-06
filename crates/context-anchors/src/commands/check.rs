use std::io::Write;

use anchr_core::check::{CheckOptions, run_check};
use anchr_core::config::UnverifiedPolicy;
use anchr_core::root::FilePath;
use anyhow::{Context, bail};
use camino::Utf8Path;

use super::{Outcome, absolute, current_dir, discover};
use crate::cli::{CheckArgs, Format};
use crate::render;

pub fn run(args: &CheckArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let discovered = discover(&cwd, args.root.as_deref())?;
    let only_files = args
        .paths
        .iter()
        .map(|path| relative_to_root(&cwd, &discovered.root_dir, path))
        .collect::<anyhow::Result<Vec<FilePath>>>()?;

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

/// A `PATHS` argument, as typed from the shell, expressed relative to the discovered root.
fn relative_to_root(
    cwd: &Utf8Path,
    root_dir: &Utf8Path,
    path: &Utf8Path,
) -> anyhow::Result<FilePath> {
    let absolute = absolute(cwd, path);
    let Ok(relative) = absolute.strip_prefix(root_dir) else {
        bail!("{path} is outside the root {root_dir}");
    };
    FilePath::new(relative.to_path_buf())
        .with_context(|| format!("{path} is not a file path inside the root"))
}
