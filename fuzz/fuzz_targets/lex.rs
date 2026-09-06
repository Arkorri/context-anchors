#![no_main]

use anchr_core::marker::{MarkerPayload, lex};
use anchr_core::text::{RegionKind, TextRegions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let regions = TextRegions::whole(text.len(), RegionKind::Whole);
        let lexed = lex(text, &regions).expect("a whole-file region is always on a char boundary");
        for marker in &lexed.markers {
            assert!(text.get(marker.span.start..marker.span.end).is_some());
            assert!(text.get(marker.body_span.start..marker.body_span.end).is_some());
            if let MarkerPayload::Ref {
                alias: Some(declared),
                ..
            } = &marker.payload
            {
                assert!(text.get(declared.span.start..declared.span.end).is_some());
            }
        }
    }
});
