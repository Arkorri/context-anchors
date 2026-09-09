use super::*;

#[test]
fn accepts_paths_symbols_directories_and_globs() {
    for raw in [
        "foo.ts",
        "src/file.ts#Name",
        "some/dir/",
        "CLAUDE.md",
        "anchr",
        "Ünïcode.md",
        "src/**",
        "*.md",
        "**/CLAUDE.md",
        "docs/[ab].md",
        "{a,b}.md",
        "x?.ts",
    ] {
        assert_eq!(NoRefEntry::parse(raw).unwrap().as_str(), raw);
    }
}

#[test]
fn rejects_each_malformed_shape() {
    assert_eq!(NoRefEntry::parse(""), Err(NoRefEntryError::Empty));
    assert_eq!(NoRefEntry::parse("a b"), Err(NoRefEntryError::Whitespace));
    assert_eq!(NoRefEntry::parse("a\tb"), Err(NoRefEntryError::Whitespace));
    for (raw, ch) in [("a@b", '@'), ("`a`", '`')] {
        assert_eq!(
            NoRefEntry::parse(raw),
            Err(NoRefEntryError::InvalidChar { ch }),
            "{raw}"
        );
    }
    assert!(matches!(
        NoRefEntry::parse("docs/["),
        Err(NoRefEntryError::Glob { .. })
    ));
    let longest = "a".repeat(MAX_NOREF_ENTRY_BYTES);
    assert!(NoRefEntry::parse(&longest).is_ok());
    assert_eq!(
        NoRefEntry::parse(&format!("{longest}a")),
        Err(NoRefEntryError::TooLong {
            len: MAX_NOREF_ENTRY_BYTES + 1
        })
    );
}

#[test]
fn entries_compare_by_their_text() {
    let a = NoRefEntry::parse("a.md").unwrap();
    assert_eq!(a, NoRefEntry::parse("a.md").unwrap());
    assert_ne!(a, NoRefEntry::parse("b.md").unwrap());
    assert!(a < NoRefEntry::parse("b.md").unwrap());
}

#[test]
fn a_list_yields_entries_with_body_relative_spans() {
    let body = "a, b/,c.ts#Name,\td";
    let items = parse_noref_body(body).unwrap();
    let described: Vec<(&str, &str)> = items
        .iter()
        .map(|item| (item.entry.as_str(), &body[item.span.start..item.span.end]))
        .collect();
    assert_eq!(
        described,
        vec![
            ("a", "a"),
            ("b/", "b/"),
            ("c.ts#Name", "c.ts#Name"),
            ("d", "d")
        ]
    );
    assert_eq!(items[3].span, ByteSpan::new(17, 18));
}

#[test]
fn a_single_entry_spans_the_whole_body() {
    let items = parse_noref_body("docs/x.md").unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].span, ByteSpan::new(0, 9));
}

#[test]
fn rejects_each_malformed_list() {
    assert_eq!(parse_noref_body(" a"), Err(NoRefError::Padded));
    assert_eq!(parse_noref_body("a "), Err(NoRefError::Padded));
    assert_eq!(parse_noref_body("a,"), Err(NoRefError::EmptyEntry));
    assert_eq!(parse_noref_body(",a"), Err(NoRefError::EmptyEntry));
    assert_eq!(parse_noref_body("a,,b"), Err(NoRefError::EmptyEntry));
    assert_eq!(parse_noref_body("a, ,b"), Err(NoRefError::EmptyEntry));
    assert_eq!(
        parse_noref_body("a b"),
        Err(NoRefError::Entry {
            raw: "a b".to_owned(),
            reason: NoRefEntryError::Whitespace
        })
    );
    assert_eq!(
        parse_noref_body("a ,b"),
        Err(NoRefError::Entry {
            raw: "a ".to_owned(),
            reason: NoRefEntryError::Whitespace
        })
    );
    assert_eq!(
        parse_noref_body("a,`b`"),
        Err(NoRefError::Entry {
            raw: "`b`".to_owned(),
            reason: NoRefEntryError::InvalidChar { ch: '`' }
        })
    );
}
