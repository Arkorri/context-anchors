//! "Did you mean" over a candidate set. rustc's rule: a case-insensitive exact match wins;
//! otherwise the closest candidate within an edit budget of a third of the length.

use crate::marker::AnchorId;

/// At most one suggestion; `None` when nothing is close enough to be worth saying.
pub fn suggest<'a>(query: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let budget = query.chars().count().max(3) / 3;
    let lowered = query.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        if candidate == query {
            return None;
        }
        if candidate.to_lowercase() == lowered {
            return Some(candidate.to_owned());
        }
        let distance = strsim::osa_distance(query, candidate);
        if distance <= budget && best.is_none_or(|(best_distance, _)| distance < best_distance) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate.to_owned())
}

/// Anchor ids are hierarchical, so a query that matches only the last segment of a candidate
/// (`token-refresh` for `auth/token-refresh`) is still a useful suggestion.
pub fn suggest_anchor<'a>(
    query: &AnchorId,
    candidates: impl IntoIterator<Item = &'a AnchorId>,
) -> Option<String> {
    let candidates: Vec<&AnchorId> = candidates.into_iter().collect();
    if let Some(found) = suggest(query.as_str(), candidates.iter().map(|id| id.as_str())) {
        return Some(found);
    }
    let last = query.last_segment();
    candidates
        .into_iter()
        .filter(|candidate| candidate.as_str() != query.as_str())
        .find(|candidate| {
            let candidate_last = candidate.last_segment();
            candidate_last == last
                || strsim::osa_distance(last, candidate_last) <= last.chars().count().max(3) / 3
        })
        .map(|candidate| candidate.as_str().to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn exact_case_insensitive_matches_win() {
        assert_eq!(
            suggest("validatetoken", ["other", "validateToken"]).as_deref(),
            Some("validateToken")
        );
    }

    #[test]
    fn close_edits_are_suggested_within_budget() {
        assert_eq!(suggest("guid", ["guide", "grid"]).as_deref(), Some("guide"));
        assert_eq!(
            suggest("provder.ts", ["provider.ts"]).as_deref(),
            Some("provider.ts")
        );
        assert_eq!(suggest("abc", ["xyz"]), None);
        assert_eq!(suggest("abcdef", ["abxxef"]).as_deref(), Some("abxxef"));
        assert_eq!(suggest("abcdef", ["abxxxf"]), None);
    }

    #[test]
    fn transpositions_count_as_one_edit() {
        assert_eq!(suggest("recieve", ["receive"]).as_deref(), Some("receive"));
    }

    #[test]
    fn an_exact_match_yields_nothing() {
        assert_eq!(suggest("guide", ["guide"]), None);
    }

    #[test]
    fn anchor_suggestions_fall_back_to_the_last_segment() {
        let ids: Vec<AnchorId> = ["auth/token-refresh", "docs/overview"]
            .into_iter()
            .map(|id| AnchorId::parse(id).unwrap())
            .collect();
        let query = AnchorId::parse("token-refresh").unwrap();
        assert_eq!(
            suggest_anchor(&query, ids.iter()).as_deref(),
            Some("auth/token-refresh")
        );
        let typo = AnchorId::parse("auth/token-refesh").unwrap();
        assert_eq!(
            suggest_anchor(&typo, ids.iter()).as_deref(),
            Some("auth/token-refresh")
        );
        let unrelated = AnchorId::parse("payments/ledger").unwrap();
        assert_eq!(suggest_anchor(&unrelated, ids.iter()), None);
    }
}
