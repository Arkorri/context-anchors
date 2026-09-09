use super::path::is_file_relative;
use super::{
    Alias, AliasError, AnchorId, DeclaredAlias, IdError, PathError, PathExpectation, RelPath,
    SymbolError, SymbolName,
};
use crate::root::{FilePath, RootName, RootNameError, is_root_name_char};
use crate::span::ByteSpan;

/// What an `@ref[...]` body points at.
///
/// ```text
/// ref      := target [ws "as" ws alias]
/// target   := [root ":"] body
/// body     := "#" anchor_id | rel_path "#" symbol_name | rel_path ["/"]
/// rel_path := bare_path | "./" bare_path | ("../")+ bare_path
/// ```
///
/// A bare path is root-relative; `./` and `../` are relative to the file the marker is written
/// in and are anchored at parse time, so every `RelPath` here is root-relative.
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

    pub fn path(&self) -> Option<&RelPath> {
        match self {
            RefTarget::Path { path, .. } | RefTarget::Symbol { path, .. } => Some(path),
            RefTarget::Anchor { .. } => None,
        }
    }

    /// The same target aimed at another path; an anchor target is returned unchanged.
    pub fn with_path(&self, path: RelPath) -> RefTarget {
        match self {
            RefTarget::Path { root, expects, .. } => RefTarget::Path {
                root: root.clone(),
                path,
                expects: *expects,
            },
            RefTarget::Symbol { root, name, .. } => RefTarget::Symbol {
                root: root.clone(),
                path,
                name: name.clone(),
            },
            RefTarget::Anchor { .. } => self.clone(),
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
    /// The `as Alias` clause, its span relative to the start of the body.
    pub alias: Option<DeclaredAlias>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum TargetError {
    #[error("target is empty")]
    Empty,
    #[error("nothing follows the root prefix `{root}:`")]
    EmptyAfterRoot { root: String },
    #[error("nothing follows `#`; a symbol reference is written `path#Name`")]
    EmptySymbol,
    #[error("a target has no spaces; to declare an alias write `target as Alias`")]
    BadAliasClause,
    #[error(
        "`{root}:` cannot prefix a `./` or `../` path; a relative path is resolved from the file it is written in, which is always in the current root"
    )]
    RootPrefixOnRelative { root: String },
    #[error(
        "a `./` or `../` path is relative to the file it is written in; there is no file here, so write the path root-relative"
    )]
    RelativeNeedsFile,
    #[error(transparent)]
    Alias(AliasError),
    #[error(transparent)]
    Root(RootNameError),
    #[error(transparent)]
    Id(IdError),
    #[error(transparent)]
    Path(PathError),
    #[error(transparent)]
    Symbol(SymbolError),
}

/// `written_in` is the file the marker sits in, which anchors `./` and `../` paths. `None` is
/// for targets typed on a command line, where a relative path has nothing to be relative to.
pub fn parse_target(
    body: &str,
    written_in: Option<&FilePath>,
) -> Result<ParsedTarget, TargetError> {
    if body.is_empty() {
        return Err(TargetError::Empty);
    }
    if !body.contains(char::is_whitespace) {
        let (target, id_span) = parse_bare_target(body, 0, written_in)?;
        return Ok(ParsedTarget {
            target,
            id_span,
            alias: None,
        });
    }
    let padded = body.starts_with(char::is_whitespace) || body.ends_with(char::is_whitespace);
    match (whitespace_separated_tokens(body).as_slice(), padded) {
        (
            [
                (target_start, target_text),
                (_, "as"),
                (alias_start, alias_text),
            ],
            false,
        ) => {
            let (target, id_span) = parse_bare_target(target_text, *target_start, written_in)?;
            let alias = Alias::parse(alias_text).map_err(TargetError::Alias)?;
            Ok(ParsedTarget {
                target,
                id_span,
                alias: Some(DeclaredAlias {
                    alias,
                    span: ByteSpan::new(*alias_start, alias_start + alias_text.len()),
                }),
            })
        }
        _ => Err(TargetError::BadAliasClause),
    }
}

/// Non-empty runs of non-whitespace, each with its byte offset into `body`.
fn whitespace_separated_tokens(body: &str) -> Vec<(usize, &str)> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (offset, ch) in body.char_indices() {
        if ch.is_whitespace() {
            if let Some(token_start) = start.take() {
                tokens.push((token_start, &body[token_start..offset]));
            }
        } else if start.is_none() {
            start = Some(offset);
        }
    }
    if let Some(token_start) = start {
        tokens.push((token_start, &body[token_start..]));
    }
    tokens
}

/// Parses a whitespace-free target. Spans are relative to the body; `offset` is where the
/// target token starts in it, so an id span ends at the token's end, never at the body's.
fn parse_bare_target(
    text: &str,
    offset: usize,
    written_in: Option<&FilePath>,
) -> Result<(RefTarget, Option<ByteSpan>), TargetError> {
    let (root, rest, rest_offset) = split_root_prefix(text)?;

    if let Some(id_text) = rest.strip_prefix('#') {
        let id = AnchorId::parse(id_text).map_err(TargetError::Id)?;
        let id_start = offset + rest_offset + 1;
        return Ok((
            RefTarget::Anchor { root, id },
            Some(ByteSpan::new(id_start, offset + text.len())),
        ));
    }

    if is_file_relative(rest)
        && let Some(root) = &root
    {
        return Err(TargetError::RootPrefixOnRelative {
            root: root.to_string(),
        });
    }

    if let Some((path_text, name_text)) = rest.split_once('#') {
        if name_text.is_empty() {
            return Err(TargetError::EmptySymbol);
        }
        let path = parse_path(path_text, written_in)?;
        let name = SymbolName::parse(name_text).map_err(TargetError::Symbol)?;
        return Ok((RefTarget::Symbol { root, path, name }, None));
    }

    let (path_text, expects) = match rest.strip_suffix('/') {
        Some(stripped) => (stripped, PathExpectation::Directory),
        None => (rest, PathExpectation::Any),
    };
    let path = parse_path(path_text, written_in)?;
    Ok((
        RefTarget::Path {
            root,
            path,
            expects,
        },
        None,
    ))
}

fn parse_path(text: &str, written_in: Option<&FilePath>) -> Result<RelPath, TargetError> {
    match written_in {
        Some(file) => RelPath::anchored(file.directory(), text).map_err(TargetError::Path),
        None if is_file_relative(text) => Err(TargetError::RelativeNeedsFile),
        None => RelPath::parse(text).map_err(TargetError::Path),
    }
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
mod tests;
