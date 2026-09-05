#![no_main]

use std::sync::LazyLock;
use std::time::Duration;

use anchr_core::text::{Container, FileAnalyzer, LanguageRegistry};
use libfuzzer_sys::fuzz_target;

const EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "py", "go"];

static REGISTRY: LazyLock<LanguageRegistry> =
    LazyLock::new(|| LanguageRegistry::new().expect("bundled grammars compile"));

// The first byte picks the language; the rest is the source.
fuzz_target!(|data: &[u8]| {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let extension = EXTENSIONS[usize::from(*selector) % EXTENSIONS.len()];
    let spec = REGISTRY.for_extension(extension).expect("core bundle extension");
    let mut analyzer = FileAnalyzer::new(&REGISTRY, Duration::from_secs(5));
    if let Ok(scan) = analyzer.scan(Container::Source(spec), text) {
        for marker in &scan.markers {
            assert!(text.get(marker.span.start..marker.span.end).is_some());
        }
    }
    let _ = analyzer.symbols(spec, text);
});
