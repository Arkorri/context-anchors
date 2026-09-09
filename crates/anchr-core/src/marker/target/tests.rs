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
