use std::fmt;

pub const MAX_ROOT_NAME_BYTES: usize = 64;

/// Name of a root: the namespace a reference resolves in. `[A-Za-z0-9_-]+`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RootName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum RootNameError {
    #[error("root name is empty")]
    Empty,
    #[error("root name is {len} bytes; the limit is {MAX_ROOT_NAME_BYTES}")]
    TooLong { len: usize },
    #[error("root name contains `{ch}`; only letters, digits, `_` and `-` are allowed")]
    InvalidChar { ch: char },
}

impl RootName {
    pub fn parse(raw: &str) -> Result<Self, RootNameError> {
        if raw.is_empty() {
            return Err(RootNameError::Empty);
        }
        if raw.len() > MAX_ROOT_NAME_BYTES {
            return Err(RootNameError::TooLong { len: raw.len() });
        }
        if let Some(ch) = raw.chars().find(|ch| !is_root_name_char(*ch)) {
            return Err(RootNameError::InvalidChar { ch });
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn is_root_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

impl fmt::Display for RootName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_documented_charset() {
        assert_eq!(
            RootName::parse("claude-Code_2").unwrap().as_str(),
            "claude-Code_2"
        );
    }

    #[test]
    fn rejects_empty_dots_and_slashes() {
        assert_eq!(RootName::parse(""), Err(RootNameError::Empty));
        assert_eq!(
            RootName::parse("a.b"),
            Err(RootNameError::InvalidChar { ch: '.' })
        );
        assert_eq!(
            RootName::parse("a/b"),
            Err(RootNameError::InvalidChar { ch: '/' })
        );
    }

    #[test]
    fn rejects_names_over_the_limit() {
        let long = "x".repeat(MAX_ROOT_NAME_BYTES + 1);
        assert_eq!(
            RootName::parse(&long),
            Err(RootNameError::TooLong { len: 65 })
        );
    }
}
