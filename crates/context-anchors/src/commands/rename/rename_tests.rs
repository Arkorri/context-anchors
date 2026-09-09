use std::collections::BTreeMap;

use anchr_core::marker::AnchorId;

use super::*;

fn plan(anchor_sites: usize, ref_sites: usize) -> RenamePlan {
    RenamePlan {
        old: AnchorId::parse("auth/flow").unwrap(),
        new: AnchorId::parse("auth/session").unwrap(),
        edits: BTreeMap::new(),
        anchor_sites,
        ref_sites,
    }
}

fn summary(plan: &RenamePlan, dry_run: bool) -> String {
    let mut out = Vec::new();
    write_summary(&mut out, plan, dry_run).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_dry_run_is_phrased_as_something_not_yet_done() {
    assert!(summary(&plan(1, 2), true).starts_with("would rename `auth/flow` -> `auth/session`"));
    assert!(summary(&plan(1, 2), false).starts_with("renamed `auth/flow` -> `auth/session`"));
}

#[test]
fn declarations_and_references_are_pluralised_independently() {
    let text = summary(&plan(1, 12), false);
    assert!(text.contains("1 declaration, 12 references"), "{text}");

    let text = summary(&plan(2, 1), false);
    assert!(text.contains("2 declarations, 1 reference"), "{text}");
}

#[test]
fn every_summary_points_at_the_command_that_confirms_it() {
    for dry_run in [true, false] {
        assert!(summary(&plan(1, 1), dry_run).contains("run `anchr check` to confirm"));
    }
}
