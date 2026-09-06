use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};

pub const MAX_PATH_BYTES: usize = 1024;

/// Characters the target grammar reserves; a path segment may not contain them.
const RESERVED: [char; 5] = ['[', ']', '#', ':', '\\'];

/// A root-relative, forward-slash path with no `.`/`..` segments and an allowlisted charset.
/// Never touches the filesystem; joining it onto a root cannot escape that root.
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
    #[error("path contains a `.` segment")]
    CurrentDirectory,
    #[error("path contains a `..` segment; paths cannot leave the root")]
    ParentDirectory,
    #[error("path contains `{ch}`, which the target grammar reserves")]
    ReservedChar { ch: char },
    #[error("path contains whitespace or a control character")]
    InvalidChar { ch: char },
}

impl RelPath {
    pub fn parse(raw: &str) -> Result<Self, PathError> {
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
        for segment in raw.split('/') {
            validate_segment(segment)?;
        }
        Ok(Self(Utf8PathBuf::from(raw)))
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
mod tests {
    use camino::Utf8Component;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn accepts_ordinary_relative_paths() {
        let path = RelPath::parse("src/auth/provider.ts").unwrap();
        assert_eq!(path.extension(), Some("ts"));
        assert_eq!(path.parent().unwrap().as_str(), "src/auth");
        assert_eq!(path.file_name(), "provider.ts");
        assert_eq!(RelPath::parse("README").unwrap().parent(), None);
        assert_eq!(
            RelPath::parse("données/façade.md").unwrap().as_str(),
            "données/façade.md"
        );
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(RelPath::parse(""), Err(PathError::Empty));
        assert_eq!(RelPath::parse("/etc/passwd"), Err(PathError::Absolute));
        assert_eq!(RelPath::parse("a\\b"), Err(PathError::Backslash));
        assert_eq!(RelPath::parse("a//b"), Err(PathError::EmptySegment));
        assert_eq!(RelPath::parse("a/"), Err(PathError::EmptySegment));
        assert_eq!(RelPath::parse("./a"), Err(PathError::CurrentDirectory));
        assert_eq!(RelPath::parse("a/../b"), Err(PathError::ParentDirectory));
        assert_eq!(
            RelPath::parse("C:/x"),
            Err(PathError::ReservedChar { ch: ':' })
        );
        assert_eq!(
            RelPath::parse("a#b"),
            Err(PathError::ReservedChar { ch: '#' })
        );
        assert_eq!(
            RelPath::parse("a b"),
            Err(PathError::InvalidChar { ch: ' ' })
        );
        assert_eq!(
            RelPath::parse("a\tb"),
            Err(PathError::InvalidChar { ch: '\t' })
        );
    }

    proptest! {
        #[test]
        fn a_parsed_path_joined_onto_a_root_never_escapes_it(raw in "\\PC{0,40}") {
            if let Ok(path) = RelPath::parse(&raw) {
                let root = Utf8Path::new("/root");
                let joined = root.join(path.as_path());
                prop_assert!(joined.starts_with(root));
                prop_assert!(joined.components().all(|component| !matches!(
                    component,
                    Utf8Component::ParentDir | Utf8Component::CurDir
                )));
            }
        }
    }
}
