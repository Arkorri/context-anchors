#![no_main]

use std::sync::LazyLock;
use std::time::Duration;

use anchr_core::root::FilePath;
use anchr_core::text::{Container, FileAnalyzer, LanguageRegistry};
use camino::Utf8PathBuf;
use libfuzzer_sys::fuzz_target;

static FILE: LazyLock<FilePath> =
    LazyLock::new(|| FilePath::new(Utf8PathBuf::from("docs/deep/fuzz.md")).expect("valid path"));

static REGISTRY: LazyLock<LanguageRegistry> = LazyLock::new(|| {
    // libfuzzer aborts in its panic hook before unwinding, but the analyzer relies on unwinding
    // to turn pulldown-cmark's known panics into errors. Uncaught panics still abort.
    drop(std::panic::take_hook());
    LanguageRegistry::new().expect("bundled grammars compile")
});

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let mut analyzer = FileAnalyzer::new(&REGISTRY, Duration::from_secs(5));
        if let Ok(scan) = analyzer.scan(Container::Markdown, text, &FILE) {
            for marker in &scan.markers {
                assert!(text.get(marker.span.start..marker.span.end).is_some());
            }
        }
    }
});
