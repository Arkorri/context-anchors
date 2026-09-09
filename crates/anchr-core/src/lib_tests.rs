use super::*;

/// @ref[./lib.rs] holds no logic — it is the crate's module list — so this states that list once, in one
/// place, rather than leaving it asserted incidentally by whichever caller happens to use each
/// module. `coverage` and `rename` have no callers inside this crate at all.
///
/// It deliberately claims no more than that. From inside the crate `pub mod` and `mod` resolve
/// identically, so this cannot check visibility; that is covered for 13 of the 16 by the binary
/// crate consuming them as `anchr_core::…`, and not covered anywhere for `noref`, `suggest` and
/// `tree`.
#[test]
fn every_module_the_crate_root_declares_is_reachable() {
    fn nameable<T>() {}

    nameable::<check::CheckOptions>();
    nameable::<config::ScanConfig>();
    nameable::<coverage::CandidateSite>();
    nameable::<diagnostic::Severity>();
    nameable::<edit::TextEdit>();
    nameable::<index::Site>();
    nameable::<marker::Marker>();
    nameable::<noref::NoRefSet>();
    nameable::<rename::RenamePlan>();
    nameable::<resolve::Resolution>();
    nameable::<root::RootName>();
    nameable::<scan::ScannedFile>();
    nameable::<span::ByteSpan>();
    nameable::<text::TextRegion>();
    nameable::<tree::FileTree>();

    // `suggest` exposes functions rather than a type, and is generic, so it is named by a call.
    let _: Option<String> = suggest::suggest("anchor", ["anchors"]);
}
