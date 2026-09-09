use std::io::Write;

use anchr_core::check::{Workspace, locate};
use anchr_core::diagnostic::LocatedSite;
use anchr_core::marker::parse_target;
use anyhow::Context;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod backrefs_tests;

use super::{Outcome, current_dir, discover};
use crate::cli::{BackrefsArgs, Format};
use crate::render;

pub fn run(args: &BackrefsArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let target = parse_target(&args.target, None)
        .with_context(|| format!("`{}` is not a valid reference target", args.target))?
        .target;
    let cwd = current_dir()?;
    let workspace = Workspace::load(discover(&cwd, args.root.as_deref())?)?;
    let (_, index) = workspace.current();

    let mut sites = index
        .backrefs(&target)
        .map(|reference| locate(index, reference.site))
        .collect::<Result<Vec<LocatedSite>, _>>()?;
    sites.sort();

    let mut stdout = anstream::stdout().lock();
    match args.format {
        Format::Human => write_human(&mut stdout, &args.target, &sites)?,
        Format::Json => render::json::write_sites(&mut stdout, &args.target, &sites)?,
    }
    stdout.flush()?;
    Ok(Outcome::Clean)
}

fn write_human(out: &mut impl Write, target: &str, sites: &[LocatedSite]) -> std::io::Result<()> {
    for located in sites {
        writeln!(out, "{}:{}", located.site.path, located.line_col)?;
    }
    writeln!(
        out,
        "{} reference{} to `{target}`",
        sites.len(),
        render::plural(sites.len())
    )
}
