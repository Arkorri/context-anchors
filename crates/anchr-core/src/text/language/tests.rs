use super::*;

#[test]
fn every_bundled_grammar_and_query_compiles() {
    let registry = LanguageRegistry::new().unwrap();
    let names: Vec<&str> = registry.languages().map(LanguageSpec::name).collect();
    assert_eq!(
        names,
        vec!["rust", "typescript", "tsx", "javascript", "python", "go"]
    );
}

#[test]
fn extensions_map_to_their_language() {
    let registry = LanguageRegistry::new().unwrap();
    for (extension, language) in [
        ("rs", "rust"),
        ("ts", "typescript"),
        ("mts", "typescript"),
        ("tsx", "tsx"),
        ("js", "javascript"),
        ("jsx", "javascript"),
        ("py", "python"),
        ("go", "go"),
    ] {
        assert_eq!(registry.for_extension(extension).unwrap().name(), language);
    }
    assert!(registry.for_extension("ex").is_none());
    assert!(
        registry.for_extension("RS").is_none(),
        "lookups are lowercase"
    );
}
