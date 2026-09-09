use anchr_core::diagnostic::{Report, Summary};
use anchr_core::root::FilePath;
use camino::Utf8PathBuf;

use crate::cli::{Color, Format};

use super::*;

fn args(strict: bool) -> CheckArgs {
    CheckArgs {
        paths: Vec::new(),
        root: None,
        format: Format::Human,
        strict,
        color: Color::Never,
    }
}

fn report_with(errors: usize, unverified: usize) -> Report {
    Report {
        diagnostics: Vec::new(),
        summary: Summary {
            errors,
            unverified,
            ..Default::default()
        },
        policy: UnverifiedPolicy::Report,
        root_dirs: std::collections::BTreeMap::new(),
    }
}

/// The exit contract: only errors fail. Inverting this is the classic silent regression, and it
/// is what `tests/cli.rs` can only observe as nine unrelated failures.
#[test]
fn only_errors_make_the_run_fail() {
    assert_eq!(outcome_for(&report_with(0, 0)), Outcome::Clean);
    assert_eq!(outcome_for(&report_with(1, 0)), Outcome::Errors);
    assert_eq!(outcome_for(&report_with(9, 0)), Outcome::Errors);
}

#[test]
fn unverified_findings_alone_do_not_fail_the_run() {
    assert_eq!(outcome_for(&report_with(0, 7)), Outcome::Clean);
    // With errors present the unverified count is irrelevant.
    assert_eq!(outcome_for(&report_with(1, 7)), Outcome::Errors);
}

#[test]
fn strict_is_what_promotes_unverified_findings_to_errors() {
    assert_eq!(options_from(&args(false), Vec::new()).unverified, None);
    assert_eq!(
        options_from(&args(true), Vec::new()).unverified,
        Some(UnverifiedPolicy::Error)
    );
}

#[test]
fn the_path_filter_is_carried_through_untouched() {
    let files = vec![FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap()];
    let options = options_from(&args(false), files.clone());
    assert_eq!(options.only_files, files);
}
