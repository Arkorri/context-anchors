#![no_main]

use std::sync::LazyLock;

use anchr_core::root::FilePath;
use camino::{Utf8Path, Utf8PathBuf};
use libfuzzer_sys::fuzz_target;

static FILE: LazyLock<FilePath> =
    LazyLock::new(|| FilePath::new(Utf8PathBuf::from("docs/deep/fuzz.md")).expect("valid path"));

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = anchr_core::marker::parse_target(text, None);
        let _ = anchr_core::marker::parse_target(text, Some(&FILE));
        let _ = anchr_core::marker::AnchorId::parse(text);
        let _ = anchr_core::marker::RelPath::parse(text);
        let _ = anchr_core::marker::RelPath::anchored(Utf8Path::new("docs/deep"), text);
        let _ = anchr_core::marker::SymbolName::parse(text);
        let _ = anchr_core::root::RootName::parse(text);
        let _ = anchr_core::marker::Alias::parse(text);
        let _ = anchr_core::marker::NoRefEntry::parse(text);
        let _ = anchr_core::marker::parse_noref_body(text);
    }
});
