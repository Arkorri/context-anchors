#![no_main]

use anchr_core::config::Config;
use camino::Utf8Path;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Config::from_toml(text, Utf8Path::new("/fuzz/anchr.toml"));
    }
});
