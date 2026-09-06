#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = anchr_core::marker::parse_target(text);
        let _ = anchr_core::marker::AnchorId::parse(text);
        let _ = anchr_core::marker::RelPath::parse(text);
        let _ = anchr_core::marker::SymbolName::parse(text);
        let _ = anchr_core::root::RootName::parse(text);
        let _ = anchr_core::marker::Alias::parse(text);
    }
});
