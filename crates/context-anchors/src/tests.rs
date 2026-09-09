use anyhow::anyhow;

use super::*;

/// The 0/1/2 contract the module doc states, and that `tests/cli.rs` asserts through the process
/// exit status. `ExitCode` is opaque, so the numeric mapping is what can be pinned here.
#[test]
fn each_outcome_maps_to_its_documented_exit_code() {
    assert_eq!(exit_code(&Ok(commands::Outcome::Clean)), 0);
    assert_eq!(exit_code(&Ok(commands::Outcome::Errors)), 1);
    assert_eq!(exit_code(&Err(anyhow!("the tool itself failed"))), 2);
}

#[test]
fn the_error_code_does_not_depend_on_which_error_occurred() {
    let codes = [
        exit_code(&Err(anyhow!("reading the current directory"))),
        exit_code(&Err(anyhow!("a.md is outside the root"))),
        exit_code(&Err(anyhow!("").context("wrapped"))),
    ];
    assert!(codes.iter().all(|code| *code == EXIT_FAILURE), "{codes:?}");
}
