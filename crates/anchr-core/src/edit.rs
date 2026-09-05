//! Byte-span text edits and their application, shared by `rename` and `annotate`. Every edit
//! records what it expects to find, so a file that changed since it was scanned is refused
//! rather than corrupted.

use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;

use crate::root::FilePath;
use crate::span::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub span: ByteSpan,
    /// What the span currently holds, checked again before writing.
    pub expected: String,
    pub replacement: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{path} changed since it was scanned; run the command again")]
    FileChanged { path: FilePath },
    #[error("reading {path}: {source}")]
    Read {
        path: FilePath,
        #[source]
        source: std::io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: FilePath,
        #[source]
        source: std::io::Error,
    },
}

/// Applies edits file by file, refusing any file whose bytes no longer match. Returns the
/// files written, in order. Edits within a file must be sorted by span and non-overlapping.
pub fn apply_to_files(
    root_dir: &Utf8Path,
    edits: &BTreeMap<FilePath, Vec<TextEdit>>,
) -> Result<Vec<FilePath>, ApplyError> {
    let mut rewritten = Vec::with_capacity(edits.len());
    for (path, file_edits) in edits {
        let absolute = root_dir.join(path.as_path());
        let source = fs::read_to_string(&absolute).map_err(|source| ApplyError::Read {
            path: path.clone(),
            source,
        })?;
        let updated = apply_edits(&source, file_edits)
            .ok_or_else(|| ApplyError::FileChanged { path: path.clone() })?;
        fs::write(&absolute, updated).map_err(|source| ApplyError::Write {
            path: path.clone(),
            source,
        })?;
        rewritten.push(path.clone());
    }
    Ok(rewritten)
}

/// `None` when any edit's expected text is not where it was planned to be.
pub fn apply_edits(source: &str, edits: &[TextEdit]) -> Option<String> {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        let current = source.get(edit.span.start..edit.span.end)?;
        if current != edit.expected || edit.span.start < cursor {
            return None;
        }
        output.push_str(source.get(cursor..edit.span.start)?);
        output.push_str(&edit.replacement);
        cursor = edit.span.end;
    }
    output.push_str(source.get(cursor..)?);
    Some(output)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_in_order_with_length_changes() {
        let source = "@anchor[ab] @ref[#ab] tail";
        let edits = vec![
            TextEdit {
                span: ByteSpan::new(8, 10),
                expected: "ab".into(),
                replacement: "longer-name".into(),
            },
            TextEdit {
                span: ByteSpan::new(18, 20),
                expected: "ab".into(),
                replacement: "longer-name".into(),
            },
        ];
        assert_eq!(
            apply_edits(source, &edits).unwrap(),
            "@anchor[longer-name] @ref[#longer-name] tail"
        );
    }

    #[test]
    fn a_mismatch_or_overlap_refuses_the_whole_file() {
        let source = "@anchor[ab]";
        let stale = vec![TextEdit {
            span: ByteSpan::new(8, 10),
            expected: "zz".into(),
            replacement: "x".into(),
        }];
        assert_eq!(apply_edits(source, &stale), None);
        let overlapping = vec![
            TextEdit {
                span: ByteSpan::new(0, 5),
                expected: "@anch".into(),
                replacement: "".into(),
            },
            TextEdit {
                span: ByteSpan::new(3, 6),
                expected: "cho".into(),
                replacement: "".into(),
            },
        ];
        assert_eq!(apply_edits(source, &overlapping), None);
    }
}
