use anchr_core::coverage::{CoverageReport, CoverageSummary};
use anchr_core::index::Index;
use anchr_core::root::RootName;

use super::*;

fn empty_report() -> CoverageReport {
    CoverageReport {
        candidates: Vec::new(),
        unused_config_ignores: Vec::new(),
        summary: CoverageSummary {
            annotated_refs: 2,
            ..Default::default()
        },
    }
}

fn rendered(format: Format) -> String {
    let index = Index::new(RootName::parse("r").unwrap());
    let mut out = Vec::new();
    write_report(&mut out, &index, &empty_report(), format).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn the_json_format_produces_parseable_json() {
    let text = rendered(Format::Json);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(value.get("schema").is_some(), "{text}");
}

#[test]
fn the_human_format_produces_prose_rather_than_json() {
    let text = rendered(Format::Human);
    assert!(
        serde_json::from_str::<serde_json::Value>(&text).is_err(),
        "{text}"
    );
    assert!(
        text.contains("reference-shaped strings are annotated"),
        "{text}"
    );
}
