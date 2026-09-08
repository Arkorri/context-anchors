//! Strings the author has declared are not references, and the matching that suppresses them.

use globset::GlobMatcher;

use crate::marker::NoRefEntry;

/// A set of ignore entries that remembers which ones were ever claimed, so unused entries can be
/// reported. Duplicates are kept: the second copy can never claim anything and is reported.
#[derive(Debug, Clone, Default)]
pub struct NoRefSet {
    entries: Vec<NoRefEntry>,
    matchers: Vec<GlobMatcher>,
    claimed: Vec<bool>,
}

impl NoRefSet {
    pub fn new(entries: impl IntoIterator<Item = NoRefEntry>) -> Self {
        let entries: Vec<NoRefEntry> = entries.into_iter().collect();
        let matchers = entries.iter().map(NoRefEntry::matcher).collect();
        let claimed = vec![false; entries.len()];
        Self {
            entries,
            matchers,
            claimed,
        }
    }

    /// Marks the first entry that matches `token` and reports whether one did.
    pub fn claim(&mut self, token: &str) -> bool {
        let Some(index) = self
            .matchers
            .iter()
            .position(|matcher| matches(matcher, token))
        else {
            return false;
        };
        self.claimed[index] = true;
        true
    }

    /// Entries that never matched, with their position in declaration order.
    pub fn unused(&self) -> impl Iterator<Item = (usize, &NoRefEntry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.claimed[*index])
    }
}

/// An entry matches a token when its glob matches the whole token or, for a `path#symbol` token,
/// the whole path. Nothing is implied: `src/` is only `src/`, `src/**` is the subtree.
fn matches(matcher: &GlobMatcher, token: &str) -> bool {
    matcher.is_match(token)
        || token
            .split_once('#')
            .is_some_and(|(path, _)| matcher.is_match(path))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
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
}
