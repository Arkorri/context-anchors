use super::*;

/// `is_linguist_extension` binary-searches, so an unsorted or duplicated table does not fail — it
/// returns the wrong answer for entries the disorder hides. The table is generated, which is
/// exactly why the invariant its lookup depends on is asserted here rather than assumed.
#[test]
fn the_table_is_sorted_and_free_of_duplicates() {
    assert!(!LINGUIST_EXTENSIONS.is_empty());
    assert!(LINGUIST_EXTENSIONS.is_sorted());
    assert!(
        LINGUIST_EXTENSIONS
            .windows(2)
            .all(|pair| pair[0] != pair[1])
    );
}

#[test]
fn entries_are_bare_lowercase_extensions_carrying_no_leading_dot() {
    for entry in LINGUIST_EXTENSIONS {
        assert!(!entry.is_empty(), "empty entry");
        assert!(!entry.starts_with('.'), "{entry}");
        assert!(
            entry.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-')
            }),
            "{entry}"
        );
    }
}

#[test]
fn known_extensions_are_found_and_everything_else_is_not() {
    for extension in ["rs", "md", "toml", "txt", "yml", "go", "py"] {
        assert!(is_linguist_extension(extension), "{extension}");
    }
    // Uppercase, all-digit and multi-dot names are dropped by the generator, so the table is the
    // lowercased single-segment set and nothing else.
    for extension in ["", "RS", "1", "lock", "rs.in"] {
        assert!(!is_linguist_extension(extension), "{extension}");
    }
}
