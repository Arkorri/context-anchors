use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "anchr",
    version,
    about = "Checks @anchor / @ref markers in docs, agent context files, and code comments",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Resolve every reference in the current root and report what does not resolve.
    Check(CheckArgs),
    /// List every reference to a target.
    Backrefs(BackrefsArgs),
    /// Rename an anchor id, rewriting its declaration and every reference to it.
    Rename(RenameArgs),
    /// Write an `anchr.toml`, the marker guide for agents, and optional editor hooks.
    Init(InitArgs),
    /// Run a Language Server Protocol server over stdio.
    Lsp,
    /// Print a shell completion script.
    Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Report only references in these files (root-wide findings are always reported).
    pub paths: Vec<Utf8PathBuf>,

    /// Directory to start root discovery from (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub root: Option<Utf8PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,

    /// Treat every unverified finding as an error.
    #[arg(long)]
    pub strict: bool,

    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,
}

#[derive(Debug, Args)]
pub struct BackrefsArgs {
    /// A target in reference syntax, e.g. `#auth/flow`, `src/lib.rs#run`, `docs/guide.md`.
    pub target: String,

    /// Directory to start root discovery from (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub root: Option<Utf8PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,

    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,
}

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// The anchor id to rename.
    pub old: String,

    /// The new anchor id.
    pub new: String,

    /// Directory to start root discovery from (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub root: Option<Utf8PathBuf>,

    /// Print the planned edits without writing anything.
    #[arg(long)]
    pub dry_run: bool,

    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Directory to initialize (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub root: Option<Utf8PathBuf>,

    /// Which agent integration to write besides `anchr.toml`.
    #[arg(long, value_enum, default_value_t = Agent::AgentsMd)]
    pub agent: Agent,

    /// Overwrite files that already exist with different contents.
    #[arg(long)]
    pub force: bool,

    /// Print what would be written without writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    pub shell: clap_complete::Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Color {
    Auto,
    Always,
    Never,
}

/// `AgentsMd` writes the guide agents read; `Claude` also wires a Claude Code hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Agent {
    Claude,
    AgentsMd,
    None,
}

impl From<Color> for anstream::ColorChoice {
    fn from(color: Color) -> Self {
        match color {
            Color::Auto => anstream::ColorChoice::Auto,
            Color::Always => anstream::ColorChoice::Always,
            Color::Never => anstream::ColorChoice::Never,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
