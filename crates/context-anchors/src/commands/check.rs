use std::io::Write;

use anchr_core::check::{CheckOptions, run_check};
use anchr_core::config::{self, UnverifiedPolicy};
use anchr_core::root::FilePath;
use anyhow::{Context, bail};
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

use super::Outcome;
use crate::cli::{CheckArgs, Format};
use crate::render;

pub fn run(args: &CheckArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let start = match &args.root {
        Some(root) => absolute(&cwd, root),
        None => cwd.clone(),
    };
    let discovered = config::discover(&start)?;
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

fn current_dir() -> anyhow::Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("reading the current directory")?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|path| anyhow::anyhow!("current directory {} is not valid UTF-8", path.display()))
}

/// Joins onto `cwd` and collapses `.` and `..` lexically, so containment checks see the real
/// target rather than a path that merely starts with the root's components.
fn absolute(cwd: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = Utf8PathBuf::new();
    for component in joined.components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_str()),
        }
    }
    normalized
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
