pub mod backrefs;
pub mod check;
pub mod completions;
pub mod init;
pub mod rename;

use anchr_core::config::{self, Discovered};
use anyhow::Context;
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

/// What a command wants the process to exit with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Errors,
}

pub fn current_dir() -> anyhow::Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("reading the current directory")?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|path| anyhow::anyhow!("current directory {} is not valid UTF-8", path.display()))
}

/// Joins onto `cwd` and collapses `.` and `..` lexically, so containment checks see the real
/// target rather than a path that merely starts with the root's components.
pub fn absolute(cwd: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = Utf8PathBuf::new();
    for component in joined.components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_str()),
        }
    }
    normalized
}

/// Root discovery from `--root` or the current directory.
pub fn discover(cwd: &Utf8Path, root: Option<&Utf8Path>) -> anyhow::Result<Discovered> {
    let start = match root {
        Some(root) => absolute(cwd, root),
        None => cwd.to_path_buf(),
    };
    Ok(config::discover(&start)?)
}
