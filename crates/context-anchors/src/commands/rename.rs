use std::io::Write;

use anchr_core::check::Workspace;
use anchr_core::marker::AnchorId;
use anchr_core::rename::{RenamePlan, apply_rename, plan_rename};
use anyhow::Context;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

use super::{Outcome, current_dir, discover};
use crate::cli::RenameArgs;

pub fn run(args: &RenameArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let old = AnchorId::parse(&args.old)
        .with_context(|| format!("`{}` is not a valid anchor id", args.old))?;
    let new = AnchorId::parse(&args.new)
        .with_context(|| format!("`{}` is not a valid anchor id", args.new))?;
    let cwd = current_dir()?;
    let workspace = Workspace::load(discover(&cwd, args.root.as_deref())?)?;
    let plan = plan_rename(&workspace, &old, &new)?;

    let mut stdout = anstream::stdout().lock();
    if args.dry_run {
        for (path, edits) in &plan.edits {
            writeln!(stdout, "would edit {path} ({} sites)", edits.len())?;
        }
    } else {
        let (root, _) = workspace.current();
        for path in apply_rename(&plan, &root.dir)? {
            writeln!(stdout, "edited {path}")?;
        }
    }
    write_summary(&mut stdout, &plan, args.dry_run)?;
    stdout.flush()?;
    Ok(Outcome::Clean)
}

fn write_summary(out: &mut impl Write, plan: &RenamePlan, dry_run: bool) -> std::io::Result<()> {
    writeln!(
        out,
        "{} `{}` -> `{}`: {} declaration{}, {} reference{}; run `anchr check` to confirm",
        if dry_run { "would rename" } else { "renamed" },
        plan.old,
        plan.new,
        plan.anchor_sites,
        crate::render::plural(plan.anchor_sites),
        plan.ref_sites,
        crate::render::plural(plan.ref_sites),
    )
}
