//! Strings the author has declared are not references, and the matching that suppresses them.

use crate::marker::NoRefEntry;

/// A set of ignore entries that remembers which ones were ever claimed, so unused entries can be
/// reported. Duplicates are kept: the second copy can never claim anything and is reported.
#[derive(Debug, Clone, Default)]
pub struct NoRefSet {
    entries: Vec<NoRefEntry>,
    claimed: Vec<bool>,
}

impl NoRefSet {
    pub fn new(entries: impl IntoIterator<Item = NoRefEntry>) -> Self {
        let entries: Vec<NoRefEntry> = entries.into_iter().collect();
        let claimed = vec![false; entries.len()];
        Self { entries, claimed }
    }

    /// Marks the first entry that matches `token` and reports whether one did.
    pub fn claim(&mut self, token: &str) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches(entry.as_str(), token))
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

/// An entry matches a token when it equals the whole token, equals the path of a `path#symbol`
/// token, or ends in `/` and prefixes either of those. Exact and case-sensitive otherwise.
fn matches(entry: &str, token: &str) -> bool {
    if entry == token {
        return true;
    }
    let path = token.split_once('#').map(|(path, _)| path);
    if path == Some(entry) {
        return true;
    }
    entry.ends_with('/') && (token.starts_with(entry) || path.is_some_and(|p| p.starts_with(entry)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn set(entries: &[&str]) -> NoRefSet {
        NoRefSet::new(entries.iter().map(|e| NoRefEntry::parse(e).unwrap()))
    }

    #[test]
    fn matches_whole_tokens_paths_of_symbols_and_directory_prefixes() {
        assert!(matches("foo.ts", "foo.ts"));
        assert!(matches("src/file.ts", "src/file.ts#Name"));
        assert!(matches("src/", "src/file.ts"));
        assert!(matches("src/", "src/file.ts#Name"));
        assert!(matches("src/", "src/deep/file.ts"));

        assert!(!matches("Name", "src/file.ts#Name"));
        assert!(!matches("src", "src/file.ts"));
        assert!(!matches("src/", "src2/file.ts"));
        assert!(!matches("foo", "foo.ts"));
        assert!(!matches("Foo.ts", "foo.ts"));
        assert!(!matches("src/file.ts#Name", "src/file.ts"));
    }

    #[test]
    fn claiming_marks_the_first_match_and_leaves_duplicates_unused() {
        let mut set = set(&["a.md", "src/", "a.md"]);
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
