mod cli;
mod commands;
mod render;

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
        Command::Init(args) => commands::init::run(&args),
        Command::Completions(args) => commands::completions::run(&args),
    };
    match outcome {
        Ok(commands::Outcome::Clean) => ExitCode::from(EXIT_CLEAN),
        Ok(commands::Outcome::Errors) => ExitCode::from(EXIT_ERRORS),
        Err(error) => {
            anstream::eprintln!("error: {error:#}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}
