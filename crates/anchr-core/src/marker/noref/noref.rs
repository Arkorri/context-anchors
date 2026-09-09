use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use globset::{Glob, GlobBuilder, GlobMatcher};

use crate::span::ByteSpan;

pub const MAX_NOREF_ENTRY_BYTES: usize = 256;

/// One glob that `@noref[...]` or `[ignore] tokens` declares matches no reference.
///
/// Matched against whole coverage tokens, never resolved. `*` stays inside one path segment and
/// `**` crosses them, so `src/` is only the bare token and `src/**` is the subtree. Whitespace,
/// `@`, and backticks are rejected because no token contains them; `[` `]` `,` are legal in a
/// config entry and simply cannot be written inside a marker body.
#[derive(Debug, Clone)]
pub struct NoRefEntry {
    raw: String,
    glob: Glob,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum NoRefEntryError {
    #[error("entry is empty")]
    Empty,
    #[error("entry is {len} bytes; the limit is {MAX_NOREF_ENTRY_BYTES}")]
    TooLong { len: usize },
    #[error("entries contain no whitespace; separate entries with commas")]
    Whitespace,
    #[error("entry contains `{ch}`, which cannot appear in an ignored string")]
    InvalidChar { ch: char },
    #[error("entry is not a valid glob: {reason}")]
    Glob { reason: String },
}

impl NoRefEntry {
    pub fn parse(raw: &str) -> Result<Self, NoRefEntryError> {
        if raw.is_empty() {
            return Err(NoRefEntryError::Empty);
        }
        if raw.len() > MAX_NOREF_ENTRY_BYTES {
            return Err(NoRefEntryError::TooLong { len: raw.len() });
        }
        if raw.contains(char::is_whitespace) {
            return Err(NoRefEntryError::Whitespace);
        }
        if let Some(ch) = raw.chars().find(|ch| matches!(ch, '@' | '`')) {
            return Err(NoRefEntryError::InvalidChar { ch });
        }
        // Explicit because globset picks the escape default per platform.
        let glob = GlobBuilder::new(raw)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|error| NoRefEntryError::Glob {
                reason: error.kind().to_string(),
            })?;
        Ok(Self {
            raw: raw.to_owned(),
            glob,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn matcher(&self) -> GlobMatcher {
        self.glob.compile_matcher()
    }
}

impl PartialEq for NoRefEntry {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for NoRefEntry {}

impl Hash for NoRefEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl PartialOrd for NoRefEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NoRefEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl fmt::Display for NoRefEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// One entry of a `@noref[...]` list with the span of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoRefItem {
    pub entry: NoRefEntry,
    pub span: ByteSpan,
}

impl NoRefItem {
    pub fn shifted_by(self, offset: usize) -> Self {
        Self {
            entry: self.entry,
            span: self.span.shifted_by(offset),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum NoRefError {
    #[error("leading or trailing whitespace")]
    Padded,
    #[error("empty entry; remove the extra comma")]
    EmptyEntry,
    #[error("`{raw}`: {reason}")]
    Entry {
        raw: String,
        reason: NoRefEntryError,
    },
}

/// Parses the body of `@noref[a, b/, c.ts]`. Entries are separated by commas; whitespace is
/// allowed only directly after a comma. Spans are relative to the body.
pub fn parse_noref_body(body: &str) -> Result<Vec<NoRefItem>, NoRefError> {
    if body.starts_with(char::is_whitespace) || body.ends_with(char::is_whitespace) {
        return Err(NoRefError::Padded);
    }
    let mut items = Vec::new();
    let mut piece_start = 0;
    for (index, piece) in body.split(',').enumerate() {
        let leading = if index == 0 {
            0
        } else {
            piece.len() - piece.trim_start().len()
        };
        let raw = &piece[leading..];
        let start = piece_start + leading;
        if raw.is_empty() {
            return Err(NoRefError::EmptyEntry);
        }
        let entry = NoRefEntry::parse(raw).map_err(|reason| NoRefError::Entry {
            raw: raw.to_owned(),
            reason,
        })?;
        items.push(NoRefItem {
            entry,
            span: ByteSpan::new(start, start + raw.len()),
        });
        piece_start += piece.len() + 1;
    }
    Ok(items)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod noref_tests;
