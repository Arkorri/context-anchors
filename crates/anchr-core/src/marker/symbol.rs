use std::fmt;

pub const MAX_SYMBOL_BYTES: usize = 256;

/// An unqualified declaration name: `[A-Za-z_$][A-Za-z0-9_$]*`.
///
/// Qualified forms (`Foo::bar`, `Class.method`) are rejected rather than matched on their last
/// segment: the guarantee is file-scoped, and a qualified name would be a guaranteed miss.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum SymbolError {
    #[error("symbol name is empty")]
    Empty,
    #[error("symbol name is {len} bytes; the limit is {MAX_SYMBOL_BYTES}")]
    TooLong { len: usize },
    #[error(
        "symbol name is qualified with `{separator}`; a reference asserts that a declaration \
         with this name exists somewhere in the file, so give the unqualified name"
    )]
    Qualified { separator: &'static str },
    #[error("symbol name starts with `{ch}`; names start with a letter, `_`, or `$`")]
    InvalidStart { ch: char },
    #[error("symbol name contains `{ch}`; only letters, digits, `_`, and `$` are allowed")]
    InvalidChar { ch: char },
}

impl SymbolName {
    pub fn parse(raw: &str) -> Result<Self, SymbolError> {
        if raw.len() > MAX_SYMBOL_BYTES {
            return Err(SymbolError::TooLong { len: raw.len() });
        }
        for separator in ["::", "."] {
            if raw.contains(separator) {
                return Err(SymbolError::Qualified { separator });
            }
        }
        let mut chars = raw.chars();
        let first = chars.next().ok_or(SymbolError::Empty)?;
        if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
            return Err(SymbolError::InvalidStart { ch: first });
        }
        if let Some(ch) = chars.find(|ch| !(ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$'))
        {
            return Err(SymbolError::InvalidChar { ch });
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SymbolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_identifier_shapes_across_the_core_languages() {
        for name in ["validateToken", "_private", "$el", "snake_case_2", "CONST"] {
            assert_eq!(SymbolName::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn rejects_qualified_names_with_a_pointer_to_the_rule() {
        assert_eq!(
            SymbolName::parse("Foo::bar"),
            Err(SymbolError::Qualified { separator: "::" })
        );
        assert_eq!(
            SymbolName::parse("Class.method"),
            Err(SymbolError::Qualified { separator: "." })
        );
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(SymbolName::parse(""), Err(SymbolError::Empty));
        assert_eq!(
            SymbolName::parse("9lives"),
            Err(SymbolError::InvalidStart { ch: '9' })
        );
        assert_eq!(
            SymbolName::parse("a-b"),
            Err(SymbolError::InvalidChar { ch: '-' })
        );
        assert_eq!(
            SymbolName::parse("a b"),
            Err(SymbolError::InvalidChar { ch: ' ' })
        );
    }
}
