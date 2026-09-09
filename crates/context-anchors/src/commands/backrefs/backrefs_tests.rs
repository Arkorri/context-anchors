use anchr_core::index::Site;
use anchr_core::root::{FilePath, RootName};
use anchr_core::span::{ByteSpan, LineCol};
use anchr_core::text::RegionKind;
use camino::Utf8PathBuf;

use super::*;

fn site(path: &str, line: u32, col: u32) -> LocatedSite {
    LocatedSite {
        site: Site {
            root: RootName::parse("r").unwrap(),
            path: FilePath::new(Utf8PathBuf::from(path)).unwrap(),
            span: ByteSpan::new(0, 4),
            region: RegionKind::Prose,
        },
        line_col: LineCol { line, col },
    }
}

fn rendered(target: &str, sites: &[LocatedSite]) -> String {
    let mut out = Vec::new();
    write_human(&mut out, target, sites).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn no_hits_still_reports_the_target_rather_than_printing_nothing() {
    assert_eq!(
        rendered("#auth/flow", &[]),
        "0 references to `#auth/flow`\n"
    );
}

#[test]
fn a_single_hit_is_singular() {
    let text = rendered("#auth/flow", &[site("a.md", 3, 5)]);
    assert!(text.contains("a.md:3:5"), "{text}");
    assert!(text.ends_with("1 reference to `#auth/flow`\n"), "{text}");
}

#[test]
fn every_site_is_listed_before_the_count() {
    let text = rendered("#x", &[site("a.md", 1, 1), site("b.md", 2, 4)]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["a.md:1:1", "b.md:2:4", "2 references to `#x`"]);
}
