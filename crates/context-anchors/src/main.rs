#[path = "cli/cli.rs"]
mod cli;
#[path = "commands/commands.rs"]
mod commands;
#[path = "lsp/lsp.rs"]
mod lsp;
#[path = "render/render.rs"]
mod render;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod main_tests;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

/// 0: clean (unverified findings may be present). 1: errors. 2: the tool itself failed.
const EXIT_CLEAN: u8 = 0;
const EXIT_ERRORS: u8 = 1;
const EXIT_FAILURE: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Check(args) => commands::check::run(&args),
        Command::Backrefs(args) => commands::backrefs::run(&args),
        Command::Rename(args) => commands::rename::run(&args),
        Command::Coverage(args) => commands::coverage::run(&args),
        Command::Annotate(args) => commands::annotate::run(&args),
        Command::Init(args) => commands::init::run(&args),
        Command::Lsp => lsp::run().map(|()| commands::Outcome::Clean),
        Command::Completions(args) => commands::completions::run(&args),
    };
    if let Err(error) = &outcome {
        anstream::eprintln!("error: {error:#}");
    }
    ExitCode::from(exit_code(&outcome))
}

fn exit_code(outcome: &anyhow::Result<commands::Outcome>) -> u8 {
    match outcome {
        Ok(commands::Outcome::Clean) => EXIT_CLEAN,
        Ok(commands::Outcome::Errors) => EXIT_ERRORS,
        Err(_) => EXIT_FAILURE,
    }
}
