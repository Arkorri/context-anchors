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
mod noref_tests;
