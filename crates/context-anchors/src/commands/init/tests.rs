use super::*;

fn settings_path() -> Utf8PathBuf {
    Utf8PathBuf::from("/repo").join(CLAUDE_SETTINGS_PATH)
}

fn merged(existing: Option<&str>) -> (String, Action) {
    merge_claude_hook(&settings_path(), existing).unwrap()
}

fn merge_error(existing: &str) -> String {
    merge_claude_hook(&settings_path(), Some(existing))
        .expect_err("the merge should refuse this file")
        .to_string()
}

#[test]
fn a_file_that_is_not_there_is_created() {
    assert_eq!(action_for(None, "body", false), Action::Create);
    assert_eq!(action_for(None, "body", true), Action::Create);
}

#[test]
fn identical_contents_are_unchanged_even_under_force() {
    assert_eq!(action_for(Some("body"), "body", false), Action::Unchanged);
    assert_eq!(action_for(Some("body"), "body", true), Action::Unchanged);
}

#[test]
fn different_contents_are_kept_unless_force_is_given() {
    assert_eq!(action_for(Some("theirs"), "ours", false), Action::Kept);
    assert_eq!(action_for(Some("theirs"), "ours", true), Action::Overwrite);
}

#[test]
fn a_dry_run_describes_what_it_would_do_without_claiming_it_did() {
    assert_eq!(verb_for(Action::Create, false), "created");
    assert_eq!(verb_for(Action::Create, true), "would create");
    assert_eq!(verb_for(Action::Overwrite, false), "overwrote");
    assert_eq!(verb_for(Action::Overwrite, true), "would overwrite");
    // Neither of these writes, so the wording does not change with --dry-run.
    assert_eq!(
        verb_for(Action::Unchanged, false),
        verb_for(Action::Unchanged, true)
    );
    assert_eq!(verb_for(Action::Kept, false), verb_for(Action::Kept, true));
    assert!(verb_for(Action::Kept, false).contains("--force"));
}

#[test]
fn a_fresh_settings_file_gets_the_hook_and_a_trailing_newline() {
    let (contents, action) = merged(None);
    assert_eq!(action, Action::Create);
    assert!(contents.ends_with('\n'));

    let value: Value = serde_json::from_str(&contents).unwrap();
    let entries = value["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["matcher"], CLAUDE_HOOK_MATCHER);
    assert_eq!(entries[0]["hooks"][0]["command"], CLAUDE_HOOK_COMMAND);
}

#[test]
fn keys_the_file_already_had_survive_the_merge() {
    let existing = r#"{"model": "opus", "hooks": {"PreToolUse": [{"matcher": "Bash"}]}}"#;
    let (contents, action) = merged(Some(existing));
    assert_eq!(action, Action::Overwrite);

    let value: Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(value["model"], "opus");
    assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(value["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
}

#[test]
fn merging_onto_its_own_output_changes_nothing() {
    let (first, _) = merged(None);
    let (second, action) = merged(Some(&first));
    assert_eq!(action, Action::Unchanged);
    assert_eq!(first, second);
}

/// Presence is a prefix match on the command, so a hook the user has since edited -- adding
/// flags, say -- is still recognised as ours and not duplicated.
#[test]
fn an_existing_anchr_hook_is_recognised_however_it_was_spelled() {
    let existing = r#"{"hooks": {"PostToolUse": [
        {"matcher": "Edit", "hooks": [{"type": "command", "command": "anchr check --strict"}]}
    ]}}"#;
    let (contents, _) = merged(Some(existing));
    let value: Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(
        value["hooks"]["PostToolUse"].as_array().unwrap().len(),
        1,
        "the hook must not be added twice"
    );
}

#[test]
fn a_foreign_post_tool_use_hook_is_kept_alongside_ours() {
    let existing = r#"{"hooks": {"PostToolUse": [
        {"matcher": "Write", "hooks": [{"type": "command", "command": "prettier --write"}]}
    ]}}"#;
    let (contents, _) = merged(Some(existing));
    let value: Value = serde_json::from_str(&contents).unwrap();
    let entries = value["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["hooks"][0]["command"], "prettier --write");
}

/// Refusing is only useful if the user can act on it, so the message carries the entry to paste.
#[test]
fn invalid_json_is_refused_with_the_entry_to_add_by_hand() {
    let message = merge_error("{not json");
    assert!(message.contains("is not valid JSON"), "{message}");
    assert!(message.contains("hooks.PostToolUse"), "{message}");
    assert!(message.contains(CLAUDE_HOOK_COMMAND), "{message}");
}

#[test]
fn settings_that_are_not_an_object_are_refused() {
    assert!(merge_error("[1, 2, 3]").contains("not an object"));
}

#[test]
fn a_hooks_key_of_the_wrong_shape_is_refused_rather_than_replaced() {
    assert!(merge_error(r#"{"hooks": []}"#).contains("`hooks` is not an object"));
    assert!(
        merge_error(r#"{"hooks": {"PostToolUse": {}}}"#)
            .contains("`hooks.PostToolUse` is not an array")
    );
}

#[test]
fn a_path_inside_the_root_is_displayed_relative_to_it() {
    let root = Utf8Path::new("/repo");
    assert_eq!(
        relative_for_display(root, Utf8Path::new("/repo/.claude/settings.json")),
        Utf8Path::new(".claude/settings.json")
    );
    // A path that is not under the root is shown in full rather than mangled.
    assert_eq!(
        relative_for_display(root, Utf8Path::new("/elsewhere/a.toml")),
        Utf8Path::new("/elsewhere/a.toml")
    );
}
