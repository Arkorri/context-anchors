use std::fmt;

pub const MAX_ID_BYTES: usize = 256;
pub const MAX_SEGMENT_BYTES: usize = 64;

/// Identity declared by `@anchor[...]`. Segments `[A-Za-z0-9_][A-Za-z0-9_.-]*` joined by `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnchorId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum IdError {
    #[error("anchor id is empty")]
    Empty,
    #[error("anchor id is {len} bytes; the limit is {MAX_ID_BYTES}")]
    TooLong { len: usize },
    #[error("anchor id has an empty segment (leading, trailing, or doubled `/`)")]
    EmptySegment,
    #[error("anchor id segment `{segment}` is over {MAX_SEGMENT_BYTES} bytes")]
    SegmentTooLong { segment: String },
    #[error("anchor id segment starts with `{ch}`; segments start with a letter, digit, or `_`")]
    InvalidSegmentStart { ch: char },
    #[error("anchor id contains `{ch}`; only letters, digits, `_`, `.`, `-`, and `/` are allowed")]
    InvalidChar { ch: char },
}

impl AnchorId {
    pub fn parse(raw: &str) -> Result<Self, IdError> {
        if raw.is_empty() {
            return Err(IdError::Empty);
        }
        if raw.len() > MAX_ID_BYTES {
            return Err(IdError::TooLong { len: raw.len() });
        }
        for segment in raw.split('/') {
            validate_segment(segment)?;
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part after the final `/`, or the whole id when it is flat.
    pub fn last_segment(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

fn validate_segment(segment: &str) -> Result<(), IdError> {
    let mut chars = segment.chars();
    let first = chars.next().ok_or(IdError::EmptySegment)?;
    if segment.len() > MAX_SEGMENT_BYTES {
        return Err(IdError::SegmentTooLong {
            segment: segment.to_owned(),
        });
    }
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return Err(IdError::InvalidSegmentStart { ch: first });
    }
    if let Some(ch) = chars.find(|ch| !is_segment_char(*ch)) {
        return Err(IdError::InvalidChar { ch });
    }
    Ok(())
}

fn is_segment_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')
}

impl fmt::Display for AnchorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_flat_and_hierarchical_ids() {
        assert_eq!(
            AnchorId::parse("ref-grammar").unwrap().as_str(),
            "ref-grammar"
        );
        let id = AnchorId::parse("auth/token-refresh.v2").unwrap();
        assert_eq!(id.last_segment(), "token-refresh.v2");
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(AnchorId::parse(""), Err(IdError::Empty));
        assert_eq!(AnchorId::parse("a//b"), Err(IdError::EmptySegment));
        assert_eq!(AnchorId::parse("/a"), Err(IdError::EmptySegment));
        assert_eq!(AnchorId::parse("a/"), Err(IdError::EmptySegment));
        assert_eq!(
            AnchorId::parse("-a"),
            Err(IdError::InvalidSegmentStart { ch: '-' })
        );
        assert_eq!(
            AnchorId::parse(".a"),
            Err(IdError::InvalidSegmentStart { ch: '.' })
        );
        assert_eq!(
            AnchorId::parse("a b"),
            Err(IdError::InvalidChar { ch: ' ' })
        );
        assert_eq!(
            AnchorId::parse("a:b"),
            Err(IdError::InvalidChar { ch: ':' })
        );
        assert_eq!(
            AnchorId::parse("é"),
            Err(IdError::InvalidSegmentStart { ch: 'é' })
        );
    }

    #[test]
    fn enforces_length_limits() {
        let long_segment = "a".repeat(MAX_SEGMENT_BYTES + 1);
        assert!(matches!(
            AnchorId::parse(&long_segment),
            Err(IdError::SegmentTooLong { .. })
        ));
        let long_id = vec!["a".repeat(MAX_SEGMENT_BYTES); 5].join("/");
        assert!(matches!(
            AnchorId::parse(&long_id),
            Err(IdError::TooLong { .. })
        ));
    }
}
