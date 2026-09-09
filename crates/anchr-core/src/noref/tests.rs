use super::*;

fn set(entries: &[&str]) -> NoRefSet {
    NoRefSet::new(entries.iter().map(|raw| NoRefEntry::parse(raw).unwrap()))
}

fn claims(entry: &str, token: &str) -> bool {
    set(&[entry]).claim(token)
}

#[test]
fn globs_match_the_whole_token_or_the_path_of_a_symbol() {
    assert!(claims("a.md", "a.md"));
    assert!(claims("a.md", "a.md#Section"));
    assert!(!claims("a.md", "docs/a.md"));
    assert!(!claims("a.md", "a.md.bak"));
    assert!(!claims("A.md", "a.md"));

    assert!(claims("src/", "src/"));
    assert!(!claims("src/", "src/x.rs"));
    assert!(!claims("src/", "src/x.rs#run"));

    for token in [
        "src/",
        "src/x.rs",
        "src/x.rs#run",
        "src/a/b.rs",
        "src/*",
        "src/**",
    ] {
        assert!(claims("src/**", token), "{token}");
    }
    assert!(!claims("src/**", "crates/x/src/lib.rs"));
    assert!(!claims("src/**", "src"));

    assert!(claims("src/*", "src/x.rs"));
    assert!(!claims("src/*", "src/a/b.rs"));

    assert!(claims("**/CLAUDE.md", "CLAUDE.md"));
    assert!(claims("**/CLAUDE.md", "docs/deep/CLAUDE.md"));
    assert!(claims("*.md", "a.md"));
    assert!(!claims("*.md", "docs/a.md"));
    assert!(claims("docs/[ab].md", "docs/a.md"));
    assert!(!claims("docs/[ab].md", "docs/c.md"));
    assert!(claims("{a,b}.md", "b.md"));
    assert!(claims("x?.ts", "x1.ts"));
    assert!(!claims("x?.ts", "x/.ts"));
}

#[test]
fn claiming_marks_the_first_match_and_leaves_duplicates_unused() {
    let mut set = set(&["a.md", "src/**", "a.md"]);
    assert!(set.claim("a.md"));
    assert!(set.claim("src/x.rs"));
    assert!(!set.claim("b.md"));
    let unused: Vec<(usize, &str)> = set.unused().map(|(i, e)| (i, e.as_str())).collect();
    assert_eq!(unused, vec![(2, "a.md")]);
}

#[test]
fn an_empty_set_claims_nothing() {
    let mut set = NoRefSet::default();
    assert!(!set.claim("anything"));
    assert_eq!(set.unused().count(), 0);
}
