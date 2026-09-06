use std::io::Write;

use anchr_core::check::{Workspace, locate};
use anchr_core::coverage::coverage;
use anchr_core::edit::apply_to_files;
use anchr_core::index::Site;

use super::{Outcome, current_dir, discover, files_in_root};
use crate::cli::AnnotateArgs;

pub fn run(args: &AnnotateArgs) -> anyhow::Result<Outcome> {
    anstream::ColorChoice::from(args.color).write_global();

    let cwd = current_dir()?;
    let discovered = discover(&cwd, args.root.as_deref())?;
    let only_files = files_in_root(&cwd, &discovered.root_dir, &args.paths)?;
    let workspace = Workspace::load(discovered)?;
    let report = coverage(&workspace, &only_files);
    let proposals = report.proposals();
    let (root, index) = workspace.current();

    let mut stdout = anstream::stdout().lock();
    for (path, edits) in &proposals {
        for edit in edits {
            let located = locate(
                index,
                Site {
                    root: root.name.clone(),
                    path: path.clone(),
                    span: edit.span,
                    region: anchr_core::text::RegionKind::Prose,
                },
            )?;
            writeln!(
                stdout,
                "{path}:{}: {} -> {}",
                located.line_col, edit.expected, edit.replacement
            )?;
        }
    }

    let count: usize = proposals.values().map(Vec::len).sum();
    if args.write {
        for path in apply_to_files(&root.dir, &proposals)? {
            writeln!(stdout, "edited {path}")?;
        }
        writeln!(
            stdout,
            "annotated {count} reference{}; run `anchr check` to confirm",
            if count == 1 { "" } else { "s" }
        )?;
    } else {
        writeln!(
            stdout,
            "{count} proposal{}; pass --write to apply",
            if count == 1 { "" } else { "s" }
        )?;
    }
    stdout.flush()?;
    Ok(Outcome::Clean)
}
