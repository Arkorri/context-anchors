use super::{AnchorId, IdError, PathError, PathExpectation, RelPath, SymbolError, SymbolName};
use crate::root::{RootName, RootNameError, is_root_name_char};
use crate::span::ByteSpan;

/// What an `@ref[...]` body points at.
///
/// ```text
/// target := [root ":"] body
/// body   := "#" anchor_id | rel_path "#" symbol_name | rel_path ["/"]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefTarget {
    Path {
        root: Option<RootName>,
        path: RelPath,
        expects: PathExpectation,
    },
    Symbol {
        root: Option<RootName>,
        path: RelPath,
        name: SymbolName,
    },
    Anchor {
        root: Option<RootName>,
        id: AnchorId,
    },
}

impl RefTarget {
    pub fn root(&self) -> Option<&RootName> {
        match self {
            RefTarget::Path { root, .. }
            | RefTarget::Symbol { root, .. }
            | RefTarget::Anchor { root, .. } => root.as_ref(),
        }
    }

    /// The same target with its root made explicit, so `#id` written in root `r` compares
    /// equal to `r:#id`.
    pub fn resolved_in(&self, written_in: &RootName) -> RefTarget {
        let root = Some(self.root().unwrap_or(written_in).clone());
        match self {
            RefTarget::Path { path, expects, .. } => RefTarget::Path {
                root,
                path: path.clone(),
                expects: *expects,
            },
            RefTarget::Symbol { path, name, .. } => RefTarget::Symbol {
                root,
                path: path.clone(),
                name: name.clone(),
            },
            RefTarget::Anchor { id, .. } => RefTarget::Anchor {
                root,
                id: id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTarget {
    pub target: RefTarget,
    /// For anchor targets, the id's byte span relative to the start of the body.
    pub id_span: Option<ByteSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum TargetError {
    #[error("target is empty")]
    Empty,
    #[error("nothing follows the root prefix `{root}:`")]
    EmptyAfterRoot { root: String },
    #[error("nothing follows `#`; a symbol reference is written `path#Name`")]
    EmptySymbol,
    #[error(transparent)]
    Root(RootNameError),
    #[error(transparent)]
    Id(IdError),
    #[error(transparent)]
    Path(PathError),
    #[error(transparent)]
    Symbol(SymbolError),
}

pub fn parse_target(body: &str) -> Result<ParsedTarget, TargetError> {
    if body.is_empty() {
        return Err(TargetError::Empty);
    }
    let (root, rest, rest_offset) = split_root_prefix(body)?;

    if let Some(id_text) = rest.strip_prefix('#') {
        let id = AnchorId::parse(id_text).map_err(TargetError::Id)?;
        let id_start = rest_offset + 1;
        return Ok(ParsedTarget {
            target: RefTarget::Anchor { root, id },
            id_span: Some(ByteSpan::new(id_start, body.len())),
        });
    }

    if let Some((path_text, name_text)) = rest.split_once('#') {
        if name_text.is_empty() {
            return Err(TargetError::EmptySymbol);
        }
        let path = RelPath::parse(path_text).map_err(TargetError::Path)?;
        let name = SymbolName::parse(name_text).map_err(TargetError::Symbol)?;
        return Ok(ParsedTarget {
            target: RefTarget::Symbol { root, path, name },
            id_span: None,
        });
    }

    let (path_text, expects) = match rest.strip_suffix('/') {
        Some(stripped) => (stripped, PathExpectation::Directory),
        None => (rest, PathExpectation::Any),
    };
    let path = RelPath::parse(path_text).map_err(TargetError::Path)?;
    Ok(ParsedTarget {
        target: RefTarget::Path {
            root,
            path,
            expects,
        },
        id_span: None,
    })
}

/// A leading `name:` is a root prefix only when `name` is root-shaped; otherwise the `:` is
/// left in place for `RelPath::parse` to reject as reserved.
fn split_root_prefix(body: &str) -> Result<(Option<RootName>, &str, usize), TargetError> {
    let Some((prefix, rest)) = body.split_once(':') else {
        return Ok((None, body, 0));
    };
    if prefix.is_empty() || !prefix.chars().all(is_root_name_char) {
        return Ok((None, body, 0));
    }
    if rest.is_empty() {
        return Err(TargetError::EmptyAfterRoot {
            root: prefix.to_owned(),
        });
    }
    let root = RootName::parse(prefix).map_err(TargetError::Root)?;
    Ok((Some(root), rest, prefix.len() + 1))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn root(name: &str) -> Option<RootName> {
        Some(RootName::parse(name).unwrap())
    }

    fn path(raw: &str) -> RelPath {
        RelPath::parse(raw).unwrap()
    }

    #[test]
    fn parses_each_target_kind() {
        assert_eq!(
            parse_target("src/dir").unwrap().target,
            RefTarget::Path {
                root: None,
                path: path("src/dir"),
                expects: PathExpectation::Any
            }
        );
        assert_eq!(
            parse_target("src/dir/").unwrap().target,
            RefTarget::Path {
                root: None,
                path: path("src/dir"),
                expects: PathExpectation::Directory
            }
        );
        assert_eq!(
            parse_target("src/file.ts#FunctionName").unwrap().target,
            RefTarget::Symbol {
                root: None,
                path: path("src/file.ts"),
                name: SymbolName::parse("FunctionName").unwrap()
            }
        );
        assert_eq!(
            parse_target("#auth/token-refresh").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: None,
                    id: AnchorId::parse("auth/token-refresh").unwrap()
                },
                id_span: Some(ByteSpan::new(1, 19)),
            }
        );
    }

    #[test]
    fn root_prefix_applies_to_every_kind_and_offsets_the_id_span() {
        assert_eq!(
            parse_target("claude:#auth/flow").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: root("claude"),
                    id: AnchorId::parse("auth/flow").unwrap()
                },
                id_span: Some(ByteSpan::new(8, 17)),
            }
        );
        assert_eq!(
            parse_target("claude:skills/x.md").unwrap().target.root(),
            root("claude").as_ref()
        );
        assert_eq!(
            parse_target("claude:a.rs#f").unwrap().target.root(),
            root("claude").as_ref()
        );
    }

    #[test]
    fn colon_that_is_not_a_root_prefix_is_a_reserved_path_char() {
        assert_eq!(
            parse_target("src/foo:bar.md"),
            Err(TargetError::Path(PathError::ReservedChar { ch: ':' }))
        );
        assert_eq!(
            parse_target("#id:x"),
            Err(TargetError::Id(IdError::InvalidChar { ch: ':' }))
        );
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(parse_target(""), Err(TargetError::Empty));
        assert_eq!(
            parse_target("claude:"),
            Err(TargetError::EmptyAfterRoot {
                root: "claude".to_owned()
            })
        );
        assert_eq!(parse_target("a.rs#"), Err(TargetError::EmptySymbol));
        assert_eq!(parse_target("#"), Err(TargetError::Id(IdError::Empty)));
        assert_eq!(
            parse_target("a.rs#Foo::bar"),
            Err(TargetError::Symbol(SymbolError::Qualified {
                separator: "::"
            }))
        );
        assert_eq!(
            parse_target("dir/#Foo"),
            Err(TargetError::Path(PathError::EmptySegment))
        );
        assert_eq!(
            parse_target("../x.md"),
            Err(TargetError::Path(PathError::ParentDirectory))
        );
        assert_eq!(
            parse_target("/x.md"),
            Err(TargetError::Path(PathError::Absolute))
        );
        assert_eq!(
            parse_target("a b.md"),
            Err(TargetError::Path(PathError::InvalidChar { ch: ' ' }))
        );
    }
}
