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
