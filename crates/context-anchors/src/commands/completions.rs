use std::io;

use clap::CommandFactory;

use super::Outcome;
use crate::cli::{Cli, CompletionsArgs};

pub fn run(args: &CompletionsArgs) -> anyhow::Result<Outcome> {
    clap_complete::generate(args.shell, &mut Cli::command(), "anchr", &mut io::stdout());
    Ok(Outcome::Clean)
}
