pub mod check;
pub mod completions;
pub mod init;

/// What a command wants the process to exit with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Errors,
}
