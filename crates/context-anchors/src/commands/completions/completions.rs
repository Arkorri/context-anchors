use std::io;

use clap::CommandFactory;

use super::Outcome;
use crate::cli::{Cli, CompletionsArgs};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod completions_tests;

pub fn run(args: &CompletionsArgs) -> anyhow::Result<Outcome> {
    write_to(args.shell, &mut io::stdout());
    Ok(Outcome::Clean)
}

fn write_to(shell: clap_complete::Shell, out: &mut impl io::Write) {
    clap_complete::generate(shell, &mut Cli::command(), "anchr", out);
}
