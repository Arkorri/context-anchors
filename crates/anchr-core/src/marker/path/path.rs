use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};

pub const MAX_PATH_BYTES: usize = 1024;

/// Characters the target grammar reserves; a path segment may not contain them.
const RESERVED: [char; 5] = ['[', ']', '#', ':', '\\'];

/// A root-relative, forward-slash path with no `.`/`..` segments and an allowlisted charset.
/// Built by `parse` from a root-relative spelling or by `anchored` from a `./` or `../` one;
/// the stored path is root-relative either way. Never touches the filesystem; joining it onto
/// a root cannot escape that root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelPath(Utf8PathBuf);

/// Whether a path target ended in `/`, which additionally requires a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathExpectation {
    Any,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path is {len} bytes; the limit is {MAX_PATH_BYTES}")]
    TooLong { len: usize },
    #[error("path is absolute; paths are relative to the root")]
    Absolute,
    #[error("path contains `\\`; use `/` as the separator")]
    Backslash,
    #[error("path has an empty segment (trailing or doubled `/`)")]
    EmptySegment,
    #[error("path contains a `.` segment; `./` is allowed only at the start of a path")]
    CurrentDirectory,
    #[error("path contains a `..` segment after a name; `../` is allowed only as a leading run")]
    ParentDirectory,
    #[error("relative path leaves the root: more `../` than directories above this file")]
    EscapesRoot,
    #[error("relative path names the root directory itself; name a file or directory inside it")]
    NamesRoot,
    #[error("path contains `{ch}`, which the target grammar reserves")]
    ReservedChar { ch: char },
    #[error("path contains whitespace or a control character")]
    InvalidChar { ch: char },
}

impl RelPath {
    /// A root-relative spelling. `./` and `..` are errors here; `anchored` accepts them.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        check_shape(raw)?;
        for segment in raw.split('/') {
            validate_segment(segment)?;
        }
        Ok(Self(Utf8PathBuf::from(raw)))
    }

    /// `written` as it appears in a marker inside a file whose directory is `file_dir`
    /// (root-relative, `""` for the root). A bare spelling is root-relative and goes through
    /// `parse`; `./x` and `../x` are anchored to `file_dir` and normalized lexically, so the
    /// result is root-relative either way and a `RelPath` never holds a `.` or `..`.
    pub fn anchored(file_dir: &Utf8Path, written: &str) -> Result<Self, PathError> {
        if !is_file_relative(written) {
            return Self::parse(written);
        }
        check_shape(written)?;
        let mut segments = written.split('/').peekable();
        if segments.peek() == Some(&".") {
            segments.next();
        } else {
            while segments.peek() == Some(&"..") {
                segments.next();
            }
        }
        for segment in segments {
            validate_segment(segment)?;
        }
        let normalized =
            crate::tree::normalize(&file_dir.join(written)).ok_or(PathError::EscapesRoot)?;
        if normalized.as_str().is_empty() {
            return Err(PathError::NamesRoot);
        }
        Ok(Self(normalized))
    }

    pub fn as_path(&self) -> &Utf8Path {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }

    pub fn parent(&self) -> Option<&Utf8Path> {
        self.0.parent().filter(|parent| !parent.as_str().is_empty())
    }

    pub fn file_name(&self) -> &str {
        self.0.file_name().unwrap_or(self.0.as_str())
    }
}

/// `.`, `..`, `./…`, `../…`. `.hidden` and `..rc` are ordinary names.
pub(crate) fn is_file_relative(text: &str) -> bool {
    text == "." || text == ".." || text.starts_with("./") || text.starts_with("../")
}

fn check_shape(raw: &str) -> Result<(), PathError> {
    if raw.is_empty() {
        return Err(PathError::Empty);
    }
    if raw.len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong { len: raw.len() });
    }
    if raw.starts_with('/') {
        return Err(PathError::Absolute);
    }
    if raw.contains('\\') {
        return Err(PathError::Backslash);
    }
    Ok(())
}

fn validate_segment(segment: &str) -> Result<(), PathError> {
    match segment {
        "" => return Err(PathError::EmptySegment),
        "." => return Err(PathError::CurrentDirectory),
        ".." => return Err(PathError::ParentDirectory),
        _ => {}
    }
    for ch in segment.chars() {
        if RESERVED.contains(&ch) {
            return Err(PathError::ReservedChar { ch });
        }
        if ch.is_whitespace() || ch.is_control() {
            return Err(PathError::InvalidChar { ch });
        }
    }
    Ok(())
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod path_tests;
