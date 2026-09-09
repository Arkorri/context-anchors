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
mod tests;
