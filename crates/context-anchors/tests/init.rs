//! `anchr init`: writes config and guide, wires the Claude Code hook, and is idempotent.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

fn anchr_in(dir: &std::path::Path) -> Command {
    let mut command = Command::cargo_bin("anchr").unwrap();
    command.current_dir(dir);
    command
}

#[test]
fn init_writes_config_and_guide_and_is_a_no_op_the_second_time() {
    let dir = tempfile::tempdir().unwrap();
    anchr_in(dir.path())
        .args(["init"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("created: anchr.toml"))
        .stdout(predicate::str::contains("created: ANCHR.md"))
        .stdout(predicate::str::contains("AGENTS.md"));
    assert!(dir.path().join("anchr.toml").is_file());
    assert!(dir.path().join("ANCHR.md").is_file());
    assert!(!dir.path().join(".claude").exists());

    anchr_in(dir.path())
        .args(["init"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("unchanged: anchr.toml"))
        .stdout(predicate::str::contains("unchanged: ANCHR.md"));
}

#[test]
fn the_written_config_is_valid_and_the_guide_passes_its_own_check() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    anchr_in(&project).args(["init"]).assert().code(0);
    anchr_in(&project)
        .args(["check", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("0 errors, 0 unverified"));
}

#[test]
fn existing_files_are_kept_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("anchr.toml"), "# mine\n").unwrap();
    anchr_in(dir.path())
        .args(["init", "--agent", "none"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains(
            "kept (differs; pass --force to overwrite): anchr.toml",
        ));
    assert_eq!(
        fs::read_to_string(dir.path().join("anchr.toml")).unwrap(),
        "# mine\n"
    );

    anchr_in(dir.path())
        .args(["init", "--agent", "none", "--force"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("overwrote: anchr.toml"));
    assert!(
        fs::read_to_string(dir.path().join("anchr.toml"))
            .unwrap()
            .contains("[roots]")
    );
}

#[test]
fn dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    anchr_in(dir.path())
        .args(["init", "--agent", "claude", "--dry-run"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("would create: anchr.toml"))
        .stdout(predicate::str::contains(
            "would create: .claude/settings.json",
        ));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn claude_settings_are_merged_preserving_foreign_keys_and_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let settings_path = dir.path().join(".claude/settings.json");
    fs::write(
        &settings_path,
        r#"{
  "permissions": { "allow": ["Bash(ls:*)"] },
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo hi" }] }]
  }
}
"#,
    )
    .unwrap();

    anchr_in(dir.path())
        .args(["init", "--agent", "claude"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("overwrote: .claude/settings.json"));

    let merged: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(merged["permissions"]["allow"][0], "Bash(ls:*)");
    assert_eq!(
        merged["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "echo hi"
    );
    let post = merged["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 1);
    assert!(
        post[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("anchr check")
    );

    anchr_in(dir.path())
        .args(["init", "--agent", "claude"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("unchanged: .claude/settings.json"));
    let again: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(again["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
}

#[test]
fn invalid_claude_settings_are_refused_with_the_entry_to_paste() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(dir.path().join(".claude/settings.json"), "{ not json").unwrap();
    anchr_in(dir.path())
        .args(["init", "--agent", "claude"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not valid JSON"))
        .stderr(predicate::str::contains("PostToolUse"))
        .stderr(predicate::str::contains("anchr check"));
    assert!(
        !dir.path().join("anchr.toml").exists(),
        "nothing is written when planning fails"
    );
}

#[test]
fn init_accepts_a_root_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("project");
    fs::create_dir(&target).unwrap();
    anchr_in(dir.path())
        .args(["init", "--agent", "none", "--root", "project"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("created: anchr.toml"));
    assert!(target.join("anchr.toml").is_file());
}
