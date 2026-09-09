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
