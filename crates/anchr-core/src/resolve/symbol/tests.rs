use std::time::Duration;

use camino::Utf8PathBuf;

use crate::config::Config;
use crate::text::LanguageRegistry;

use super::*;

fn root_at(dir: &Utf8Path) -> Root {
    Root {
        name: RootName::parse("r").unwrap(),
        dir: dir.to_path_buf(),
        config: Config::default(),
    }
}

fn rel(path: &str) -> RelPath {
    RelPath::parse(path).unwrap()
}

fn name() -> RootName {
    RootName::parse("r").unwrap()
}

/// `FileSymbols` is not `Debug`, so failures are described rather than formatted.
fn expect_err(loaded: Result<FileSymbols, Unverified>) -> Unverified {
    match loaded {
        Ok(_) => panic!("expected the load to fail, but it succeeded"),
        Err(unverified) => unverified,
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root_dir: Utf8PathBuf,
    registry: LanguageRegistry,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        for (path, contents) in files {
            let full = root_dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        Self {
            _dir: dir,
            root_dir,
            registry: LanguageRegistry::new().unwrap(),
        }
    }

    fn load(&self, root: &Root, path: &str) -> Result<FileSymbols, Unverified> {
        let mut analyzer = FileAnalyzer::new(&self.registry, Duration::from_secs(5));
        super::load(root, &rel(path), &self.root_dir.join(path), &mut analyzer)
    }

    fn root(&self) -> Root {
        root_at(&self.root_dir)
    }
}

/// The arms a real file cannot easily provoke: a timeout needs a pathological parse and
/// truncation needs thousands of declarations, while everything else collapses into one
/// message-carrying finding.
#[test]
fn each_analysis_failure_maps_to_its_own_finding() {
    let timeout = unverified_for(
        name(),
        rel("a.rs"),
        AnalyzeError::ParseTimeout {
            budget: Duration::from_millis(5),
        },
    );
    assert!(
        matches!(timeout, Unverified::ParseTimeout { .. }),
        "{timeout:?}"
    );

    let truncated = unverified_for(name(), rel("a.rs"), AnalyzeError::SymbolTableTruncated);
    assert!(
        matches!(truncated, Unverified::SymbolTableTruncated { .. }),
        "{truncated:?}"
    );

    for other in [AnalyzeError::ParseFailed, AnalyzeError::ParserPanicked] {
        let expected = other.to_string();
        match unverified_for(name(), rel("a.rs"), other) {
            Unverified::AnalyzeFailed { message, .. } => assert_eq!(message, expected),
            found => panic!("expected AnalyzeFailed, got {found:?}"),
        }
    }
}

#[test]
fn a_file_with_no_grammar_reports_the_extension_it_could_not_handle() {
    let fixture = Fixture::new(&[("a.ex", "defmodule X do end")]);
    match expect_err(fixture.load(&fixture.root(), "a.ex")) {
        Unverified::NoGrammar { extension, .. } => assert_eq!(extension, Some("ex".to_owned())),
        found => panic!("expected NoGrammar, got {found:?}"),
    }
}

#[test]
fn an_extension_is_matched_case_insensitively() {
    let fixture = Fixture::new(&[("A.RS", "pub fn thing() {}")]);
    let symbols = fixture.load(&fixture.root(), "A.RS").unwrap();
    assert_eq!(symbols.language, "rust");
    assert!(symbols.names().any(|name| name == "thing"));
}

#[test]
fn a_file_with_no_extension_reports_no_grammar_without_one() {
    let fixture = Fixture::new(&[("LICENSE", "text")]);
    match expect_err(fixture.load(&fixture.root(), "LICENSE")) {
        Unverified::NoGrammar { extension, .. } => assert_eq!(extension, None),
        found => panic!("expected NoGrammar, got {found:?}"),
    }
}

#[test]
fn a_target_that_is_not_there_is_unreadable_rather_than_missing() {
    let fixture = Fixture::new(&[]);
    match expect_err(fixture.load(&fixture.root(), "gone.rs")) {
        Unverified::TargetUnreadable { message, .. } => assert!(!message.is_empty()),
        found => panic!("expected TargetUnreadable, got {found:?}"),
    }
}

#[test]
fn a_target_over_the_limit_reports_both_its_size_and_the_limit() {
    let fixture = Fixture::new(&[("a.rs", "pub fn thing() {}")]);
    let mut root = fixture.root();
    root.config.scan.max_file_bytes = 1;
    match expect_err(fixture.load(&root, "a.rs")) {
        Unverified::TargetTooLarge { bytes, limit, .. } => {
            assert_eq!(limit, 1);
            assert_eq!(bytes, 17);
        }
        found => panic!("expected TargetTooLarge, got {found:?}"),
    }
}

#[test]
fn a_target_that_is_not_utf8_is_reported_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let root_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    std::fs::write(root_dir.join("a.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let registry = LanguageRegistry::new().unwrap();
    let mut analyzer = FileAnalyzer::new(&registry, Duration::from_secs(5));
    let root = root_at(&root_dir);

    let loaded = super::load(&root, &rel("a.rs"), &root_dir.join("a.rs"), &mut analyzer);
    assert!(
        matches!(expect_err(loaded), Unverified::TargetNotUtf8 { .. }),
        "expected TargetNotUtf8"
    );
}

/// Failures are cached too, so a file that cannot be parsed is not re-read once per reference.
#[test]
fn a_failed_load_is_cached_and_never_reads_as_a_success() {
    let fixture = Fixture::new(&[("a.ex", "defmodule X do end")]);
    let root = fixture.root();
    let path = rel("a.ex");
    let absolute = fixture.root_dir.join("a.ex");
    let mut analyzer = FileAnalyzer::new(&fixture.registry, Duration::from_secs(5));
    let mut cache = SymbolCache::default();

    let first = cache
        .load(&root, &path, &absolute, &mut analyzer)
        .err()
        .expect("a file with no grammar cannot load");
    assert!(matches!(first, Unverified::NoGrammar { .. }));
    assert!(
        cache.cached(&root.name, &path).is_none(),
        "a cached failure is not a cached success"
    );

    // The file becomes loadable on disk, but the cached failure stands for the rest of the run.
    std::fs::write(&absolute, "pub fn thing() {}").unwrap();
    let second = cache
        .load(&root, &path, &absolute, &mut analyzer)
        .err()
        .expect("the cached failure stands");
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
}

#[test]
fn a_successful_load_is_readable_from_the_cache() {
    let fixture = Fixture::new(&[("a.rs", "pub fn thing() {}")]);
    let root = fixture.root();
    let path = rel("a.rs");
    let mut analyzer = FileAnalyzer::new(&fixture.registry, Duration::from_secs(5));
    let mut cache = SymbolCache::default();

    cache
        .load(&root, &path, &fixture.root_dir.join("a.rs"), &mut analyzer)
        .expect("a rust file loads");
    let cached = cache.cached(&root.name, &path).unwrap();
    assert_eq!(cached.language, "rust");
}
