use std::fmt;

pub const MAX_ALIAS_BYTES: usize = 64;

/// A file-local name for a target: declared by `@ref[target as Alias]`, used as `@[Alias]`.
///
/// `[A-Za-z_][A-Za-z0-9_]*`. Disjoint from the path and id charsets on purpose: an alias is a
/// name, never a target, and a diagnostic can never confuse the two.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Alias(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum AliasError {
    #[error("alias is empty")]
    Empty,
    #[error("alias is {len} bytes; the limit is {MAX_ALIAS_BYTES}")]
    TooLong { len: usize },
    #[error("alias starts with `{ch}`; aliases start with a letter or `_`")]
    InvalidStart { ch: char },
    #[error("alias contains `{ch}`; only letters, digits, and `_` are allowed")]
    InvalidChar { ch: char },
}

impl Alias {
    pub fn parse(raw: &str) -> Result<Self, AliasError> {
        if raw.len() > MAX_ALIAS_BYTES {
            return Err(AliasError::TooLong { len: raw.len() });
        }
        let mut chars = raw.chars();
        let first = chars.next().ok_or(AliasError::Empty)?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(AliasError::InvalidStart { ch: first });
        }
        if let Some(ch) = chars.find(|ch| !(ch.is_ascii_alphanumeric() || *ch == '_')) {
            return Err(AliasError::InvalidChar { ch });
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_identifier_shaped_names() {
        for name in ["Analyzer", "_private", "a1", "CONST", "UserAuth"] {
            assert_eq!(Alias::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(Alias::parse(""), Err(AliasError::Empty));
        assert_eq!(
            Alias::parse("9lives"),
            Err(AliasError::InvalidStart { ch: '9' })
        );
        assert_eq!(
            Alias::parse("a-b"),
            Err(AliasError::InvalidChar { ch: '-' })
        );
        assert_eq!(
            Alias::parse("a/b"),
            Err(AliasError::InvalidChar { ch: '/' })
        );
        assert_eq!(
            Alias::parse("a.b"),
            Err(AliasError::InvalidChar { ch: '.' })
        );
        assert_eq!(
            Alias::parse("a b"),
            Err(AliasError::InvalidChar { ch: ' ' })
        );
        assert_eq!(
            Alias::parse("$el"),
            Err(AliasError::InvalidStart { ch: '$' })
        );
    }

    #[test]
    fn enforces_the_length_limit() {
        let longest = "a".repeat(MAX_ALIAS_BYTES);
        assert!(Alias::parse(&longest).is_ok());
        assert_eq!(
            Alias::parse(&format!("{longest}a")),
            Err(AliasError::TooLong {
                len: MAX_ALIAS_BYTES + 1
            })
        );
    }
}
