use super::*;

fn region(start: usize, end: usize) -> TextRegion {
    TextRegion {
        span: ByteSpan::new(start, end),
        kind: RegionKind::Comment,
    }
}

#[test]
fn regions_are_sorted_and_nested_or_overlapping_ones_merge() {
    let regions = TextRegions::new(vec![
        region(10, 20),
        region(0, 5),
        region(12, 15),
        region(18, 25),
        region(30, 30),
    ]);
    let spans: Vec<ByteSpan> = regions.iter().map(|r| r.span).collect();
    assert_eq!(spans, vec![ByteSpan::new(0, 5), ByteSpan::new(10, 25)]);
}

#[test]
fn container_selection_is_by_lowercased_extension_and_opt_in() {
    let registry = LanguageRegistry::new().unwrap();
    let rules = ContainerRules::default();
    let select = |raw: &str| Container::for_path(Utf8Path::new(raw), &rules, &registry);
    assert_eq!(select("README.MD"), Some(Container::Markdown));
    assert_eq!(select("notes.txt"), Some(Container::Plaintext));
    assert!(matches!(select("src/lib.rs"), Some(Container::Source(spec)) if spec.name() == "rust"));
    assert_eq!(select("Makefile"), None);
    assert_eq!(select("image.png"), None);
}

#[test]
fn a_zero_budget_parse_of_a_large_file_times_out_instead_of_hanging() {
    let registry = LanguageRegistry::new().unwrap();
    let spec = registry.for_extension("rs").unwrap();
    let mut analyzer = FileAnalyzer::new(&registry, Duration::ZERO);
    let source = "fn f() { let x = 1 + 2; }\n".repeat(50_000);
    assert!(matches!(
        analyzer.symbols(spec, &source),
        Err(AnalyzeError::ParseTimeout { .. })
    ));
    // The parser is usable again after a cancelled parse.
    let mut healthy = FileAnalyzer::new(&registry, Duration::from_secs(5));
    assert!(healthy.symbols(spec, "fn f() {}").is_ok());
}
