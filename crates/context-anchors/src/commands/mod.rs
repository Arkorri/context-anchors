pub mod annotate;
pub mod backrefs;
pub mod check;
pub mod completions;
pub mod coverage;
pub mod init;
pub mod rename;

use anchr_core::config::{self, Discovered};
use anchr_core::root::FilePath;
use anyhow::{Context, bail};
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

/// `PATHS` arguments as typed from the shell, expressed relative to the discovered root.
pub fn files_in_root(
    cwd: &Utf8Path,
    root_dir: &Utf8Path,
    paths: &[Utf8PathBuf],
) -> anyhow::Result<Vec<FilePath>> {
    paths
        .iter()
        .map(|path| {
            let absolute = absolute(cwd, path);
            let Ok(relative) = absolute.strip_prefix(root_dir) else {
                bail!("{path} is outside the root {root_dir}");
            };
            FilePath::new(relative.to_path_buf())
                .with_context(|| format!("{path} is not a file path inside the root"))
        })
        .collect()
}
