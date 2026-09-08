#![no_main]

use std::sync::LazyLock;

use anchr_core::marker::{MarkerPayload, lex};
use anchr_core::root::FilePath;
use anchr_core::text::{RegionKind, TextRegions};
use camino::Utf8PathBuf;
use libfuzzer_sys::fuzz_target;

static FILE: LazyLock<FilePath> =
    LazyLock::new(|| FilePath::new(Utf8PathBuf::from("docs/deep/fuzz.md")).expect("valid path"));

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let regions = TextRegions::whole(text.len(), RegionKind::Whole);
        let lexed =
            lex(text, &regions, &FILE).expect("a whole-file region is always on a char boundary");
        for marker in &lexed.markers {
            assert!(text.get(marker.span.start..marker.span.end).is_some());
            assert!(text.get(marker.body_span.start..marker.body_span.end).is_some());
            match &marker.payload {
                MarkerPayload::Ref {
                    alias: Some(declared),
                    ..
                } => assert!(text.get(declared.span.start..declared.span.end).is_some()),
                MarkerPayload::NoRef { items } => {
                    for item in items {
                        assert_eq!(text.get(item.span.start..item.span.end), Some(item.entry.as_str()));
                    }
                }
                MarkerPayload::Anchor { .. } | MarkerPayload::Ref { .. } | MarkerPayload::Use { .. } => {}
            }
        }
    }
});
