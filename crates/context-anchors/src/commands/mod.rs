pub mod check;
pub mod completions;

/// What a command wants the process to exit with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Errors,
}
