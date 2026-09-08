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
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    /// Parsed with no file, as the command line does.
    fn parse_target_here(body: &str) -> Result<ParsedTarget, TargetError> {
        parse_target(body, None)
    }

    fn file(path: &str) -> FilePath {
        FilePath::new(Utf8PathBuf::from(path)).unwrap()
    }

    fn parse_in(body: &str, written_in: &str) -> Result<ParsedTarget, TargetError> {
        parse_target(body, Some(&file(written_in)))
    }

    fn root(name: &str) -> Option<RootName> {
        Some(RootName::parse(name).unwrap())
    }

    fn path(raw: &str) -> RelPath {
        RelPath::parse(raw).unwrap()
    }

    #[test]
    fn dot_slash_and_dot_dot_anchor_to_the_written_file() {
        let anchored = |body: &str, written_in: &str| parse_in(body, written_in).unwrap().target;
        assert_eq!(
            anchored("./x.md", "docs/a.md"),
            RefTarget::Path {
                root: None,
                path: path("docs/x.md"),
                expects: PathExpectation::Any
            }
        );
        assert_eq!(
            anchored("../x.md", "docs/design/a.md").path(),
            Some(&path("docs/x.md"))
        );
        assert_eq!(
            anchored("../../x.md", "docs/design/a.md").path(),
            Some(&path("x.md"))
        );
        assert_eq!(anchored("./x.md", "a.md").path(), Some(&path("x.md")));
        assert_eq!(
            anchored("./sub/x.ts#Name", "docs/a.md"),
            RefTarget::Symbol {
                root: None,
                path: path("docs/sub/x.ts"),
                name: SymbolName::parse("Name").unwrap()
            }
        );
        assert_eq!(
            anchored("./sub/", "docs/a.md"),
            RefTarget::Path {
                root: None,
                path: path("docs/sub"),
                expects: PathExpectation::Directory
            }
        );
        assert_eq!(
            anchored("../", "docs/design/a.md"),
            RefTarget::Path {
                root: None,
                path: path("docs"),
                expects: PathExpectation::Directory
            }
        );
        assert_eq!(
            anchored("x.md", "docs/a.md").path(),
            Some(&path("x.md")),
            "a bare path is root-relative wherever it is written"
        );
        let declared = parse_in("./x.md as X", "docs/a.md").unwrap();
        assert_eq!(declared.target.path(), Some(&path("docs/x.md")));
        assert_eq!(declared.alias, alias("X", 10));
    }

    #[test]
    fn relative_forms_reject_escapes_prefixes_and_stray_dots() {
        assert_eq!(
            parse_in("../../x.md", "docs/a.md"),
            Err(TargetError::Path(PathError::EscapesRoot))
        );
        assert_eq!(
            parse_in("./", "a.md"),
            Err(TargetError::Path(PathError::NamesRoot))
        );
        assert_eq!(
            parse_in("../", "docs/a.md"),
            Err(TargetError::Path(PathError::NamesRoot))
        );
        assert_eq!(
            parse_in("claude:./x.md", "docs/a.md"),
            Err(TargetError::RootPrefixOnRelative {
                root: "claude".to_owned()
            })
        );
        assert_eq!(
            parse_in("./a/./b", "docs/a.md"),
            Err(TargetError::Path(PathError::CurrentDirectory))
        );
        assert_eq!(
            parse_in("./../x", "docs/a.md"),
            Err(TargetError::Path(PathError::ParentDirectory))
        );
        assert_eq!(
            parse_in("a/../b", "docs/a.md"),
            Err(TargetError::Path(PathError::ParentDirectory))
        );
        assert_eq!(
            parse_in(".hidden", "docs/a.md").unwrap().target.path(),
            Some(&path(".hidden"))
        );
        assert_eq!(
            parse_in("..rc", "docs/a.md").unwrap().target.path(),
            Some(&path("..rc"))
        );
        assert_eq!(
            parse_target_here("./x.md"),
            Err(TargetError::RelativeNeedsFile)
        );
        assert_eq!(
            parse_target_here("../x.md#Name"),
            Err(TargetError::RelativeNeedsFile)
        );
    }

    #[test]
    fn parses_each_target_kind() {
        assert_eq!(
            parse_target_here("src/dir").unwrap().target,
            RefTarget::Path {
                root: None,
                path: path("src/dir"),
                expects: PathExpectation::Any
            }
        );
        assert_eq!(
            parse_target_here("src/dir/").unwrap().target,
            RefTarget::Path {
                root: None,
                path: path("src/dir"),
                expects: PathExpectation::Directory
            }
        );
        assert_eq!(
            parse_target_here("src/file.ts#FunctionName")
                .unwrap()
                .target,
            RefTarget::Symbol {
                root: None,
                path: path("src/file.ts"),
                name: SymbolName::parse("FunctionName").unwrap()
            }
        );
        assert_eq!(
            parse_target_here("#auth/token-refresh").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: None,
                    id: AnchorId::parse("auth/token-refresh").unwrap()
                },
                id_span: Some(ByteSpan::new(1, 19)),
                alias: None,
            }
        );
    }

    #[test]
    fn root_prefix_applies_to_every_kind_and_offsets_the_id_span() {
        assert_eq!(
            parse_target_here("claude:#auth/flow").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: root("claude"),
                    id: AnchorId::parse("auth/flow").unwrap()
                },
                id_span: Some(ByteSpan::new(8, 17)),
                alias: None,
            }
        );
        assert_eq!(
            parse_target_here("claude:skills/x.md")
                .unwrap()
                .target
                .root(),
            root("claude").as_ref()
        );
        assert_eq!(
            parse_target_here("claude:a.rs#f").unwrap().target.root(),
            root("claude").as_ref()
        );
    }

    #[test]
    fn colon_that_is_not_a_root_prefix_is_a_reserved_path_char() {
        assert_eq!(
            parse_target_here("src/foo:bar.md"),
            Err(TargetError::Path(PathError::ReservedChar { ch: ':' }))
        );
        assert_eq!(
            parse_target_here("#id:x"),
            Err(TargetError::Id(IdError::InvalidChar { ch: ':' }))
        );
    }

    #[test]
    fn rejects_each_malformed_shape() {
        assert_eq!(parse_target_here(""), Err(TargetError::Empty));
        assert_eq!(
            parse_target_here("claude:"),
            Err(TargetError::EmptyAfterRoot {
                root: "claude".to_owned()
            })
        );
        assert_eq!(parse_target_here("a.rs#"), Err(TargetError::EmptySymbol));
        assert_eq!(parse_target_here("#"), Err(TargetError::Id(IdError::Empty)));
        assert_eq!(
            parse_target_here("a.rs#Foo::bar"),
            Err(TargetError::Symbol(SymbolError::Qualified {
                separator: "::"
            }))
        );
        assert_eq!(
            parse_target_here("dir/#Foo"),
            Err(TargetError::Path(PathError::EmptySegment))
        );
        assert_eq!(
            parse_target_here("../x.md"),
            Err(TargetError::RelativeNeedsFile)
        );
        assert_eq!(
            parse_target_here("/x.md"),
            Err(TargetError::Path(PathError::Absolute))
        );
        assert_eq!(
            parse_target_here("a b.md"),
            Err(TargetError::BadAliasClause)
        );
    }

    fn alias(name: &str, start: usize) -> Option<DeclaredAlias> {
        Some(DeclaredAlias {
            alias: Alias::parse(name).unwrap(),
            span: ByteSpan::new(start, start + name.len()),
        })
    }

    #[test]
    fn an_alias_clause_binds_a_name_and_keeps_spans_on_the_target_token() {
        assert_eq!(
            parse_target_here("#auth/flow as Flow").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: None,
                    id: AnchorId::parse("auth/flow").unwrap()
                },
                id_span: Some(ByteSpan::new(1, 10)),
                alias: alias("Flow", 14),
            }
        );
        assert_eq!(
            parse_target_here("claude:#auth/flow as Flow").unwrap(),
            ParsedTarget {
                target: RefTarget::Anchor {
                    root: root("claude"),
                    id: AnchorId::parse("auth/flow").unwrap()
                },
                id_span: Some(ByteSpan::new(8, 17)),
                alias: alias("Flow", 21),
            }
        );
        let symbol = parse_target_here("src/x.rs#run as Run").unwrap();
        assert!(matches!(symbol.target, RefTarget::Symbol { .. }));
        assert_eq!(symbol.alias, alias("Run", 16));
        let directory = parse_target_here("docs/ as Docs").unwrap();
        assert!(matches!(
            directory.target,
            RefTarget::Path {
                expects: PathExpectation::Directory,
                ..
            }
        ));
        assert_eq!(
            parse_target_here("a.md\tas\tA").unwrap().alias,
            alias("A", 8)
        );
        assert_eq!(
            parse_target_here("a.md  as   A").unwrap().alias,
            alias("A", 11)
        );
    }

    #[test]
    fn as_inside_a_path_is_not_a_clause() {
        let parsed = parse_target_here("src/as/x.rs").unwrap();
        assert_eq!(parsed.alias, None);
        assert!(matches!(parsed.target, RefTarget::Path { .. }));
    }

    #[test]
    fn rejects_each_malformed_alias_clause() {
        for body in [
            "a.md as",
            "as A",
            "a.md as A B",
            "a.md AS A",
            " a.md",
            "a.md ",
            " a.md as A",
        ] {
            assert_eq!(
                parse_target_here(body),
                Err(TargetError::BadAliasClause),
                "{body:?}"
            );
        }
        assert_eq!(
            parse_target_here("a.md as 9a"),
            Err(TargetError::Alias(AliasError::InvalidStart { ch: '9' }))
        );
        assert_eq!(
            parse_target_here("a b.md as A"),
            Err(TargetError::BadAliasClause)
        );
        assert_eq!(
            parse_target_here("#bad id as A"),
            Err(TargetError::BadAliasClause)
        );
    }
}
