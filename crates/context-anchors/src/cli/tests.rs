use clap::CommandFactory;

use super::*;

#[test]
fn the_command_line_definition_is_consistent() {
    Cli::command().debug_assert();
}
