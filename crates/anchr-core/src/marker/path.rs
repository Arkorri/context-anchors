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

    #[test]
    fn anchored_resolves_dot_forms_against_the_file_directory() {
        let docs = Utf8Path::new("docs/design");
        let anchored = |dir: &str, written: &str| {
            RelPath::anchored(Utf8Path::new(dir), written).map(|p| p.as_str().to_owned())
        };
        assert_eq!(
            anchored("docs/design", "./x.md"),
            Ok("docs/design/x.md".to_owned())
        );
        assert_eq!(
            anchored("docs/design", "../x.md"),
            Ok("docs/x.md".to_owned())
        );
        assert_eq!(anchored("docs/design", "../../x.md"), Ok("x.md".to_owned()));
        assert_eq!(anchored("docs/design", ".."), Ok("docs".to_owned()));
        assert_eq!(
            anchored("docs", "./sub/x.md"),
            Ok("docs/sub/x.md".to_owned())
        );
        assert_eq!(anchored("", "./x.md"), Ok("x.md".to_owned()));
        assert_eq!(anchored("docs", ".hidden"), Ok(".hidden".to_owned()));
        assert_eq!(anchored("docs", "..rc"), Ok("..rc".to_owned()));
        assert_eq!(
            RelPath::anchored(docs, "x.md"),
            RelPath::parse("x.md"),
            "a bare spelling stays root-relative wherever it is written"
        );

        assert_eq!(
            anchored("docs/design", "../../../x.md"),
            Err(PathError::EscapesRoot)
        );
        assert_eq!(anchored("", "../x.md"), Err(PathError::EscapesRoot));
        assert_eq!(anchored("", "."), Err(PathError::NamesRoot));
        assert_eq!(anchored("docs", ".."), Err(PathError::NamesRoot));
        assert_eq!(
            anchored("docs", "./a/./b"),
            Err(PathError::CurrentDirectory)
        );
        assert_eq!(anchored("docs", "./../x"), Err(PathError::ParentDirectory));
        assert_eq!(
            anchored("docs", "../a/../b"),
            Err(PathError::ParentDirectory)
        );
        assert_eq!(anchored("docs", "a/../b"), Err(PathError::ParentDirectory));
        assert_eq!(anchored("docs", "./a//b"), Err(PathError::EmptySegment));
        assert_eq!(
            anchored("docs", "./a b"),
            Err(PathError::InvalidChar { ch: ' ' })
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

        #[test]
        fn an_anchored_path_never_escapes_the_root_and_matches_parse_for_bare_spellings(
            dir in "([a-z]{1,4}(/[a-z]{1,4}){0,3})?",
            raw in "\\PC{0,40}",
        ) {
            let file_dir = Utf8Path::new(&dir);
            if let Ok(path) = RelPath::anchored(file_dir, &raw) {
                let root = Utf8Path::new("/root");
                let joined = root.join(path.as_path());
                prop_assert!(joined.starts_with(root));
                prop_assert!(joined != root);
                prop_assert!(joined.components().all(|component| !matches!(
                    component,
                    Utf8Component::ParentDir | Utf8Component::CurDir
                )));
            }
            if !is_file_relative(&raw) {
                prop_assert_eq!(RelPath::anchored(file_dir, &raw), RelPath::parse(&raw));
            }
        }
    }
}
