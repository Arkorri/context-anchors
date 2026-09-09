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
