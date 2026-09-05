//! `anchr backrefs` and `anchr rename` against fixture repositories.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        Self { _dir: dir, root }
    }

    fn anchr(&self) -> Command {
        let mut command = Command::cargo_bin("anchr").unwrap();
        command.current_dir(&self.root);
        command
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
    }
}

#[test]
fn backrefs_lists_every_site_in_both_formats() {
    let fixture = Fixture::new(&[
        ("docs/a.md", "@anchor[auth/flow]\n\nSee @ref[#auth/flow].\n"),
        ("docs/b.md", "@ref[repo:#auth/flow] @ref[#other]\n"),
        ("src/x.rs", "// @ref[#auth/flow]\nfn f() {}\n"),
    ]);
    fixture
        .anchr()
        .args(["backrefs", "#auth/flow", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("docs/a.md:3:5"))
        .stdout(predicate::str::contains("docs/b.md:1:1"))
        .stdout(predicate::str::contains("src/x.rs:1:4"))
        .stdout(predicate::str::contains("3 references to `#auth/flow`"));

    let output = fixture
        .anchr()
        .args(["backrefs", "#auth/flow", "--format", "json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["target"], "#auth/flow");
    assert_eq!(json["sites"].as_array().unwrap().len(), 3);
    assert_eq!(json["sites"][2]["region"], "comment");
}

#[test]
fn backrefs_rejects_an_invalid_target() {
    let fixture = Fixture::new(&[("a.md", "")]);
    fixture
        .anchr()
        .args(["backrefs", "a b"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a valid reference target"));
}

#[test]
fn rename_rewrites_declaration_and_references_then_check_is_clean() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "# Flow @anchor[auth/flow]\n\nSee @ref[#auth/flow].\n",
        ),
        ("src/x.rs", "// @ref[#auth/flow]\nfn f() {}\n"),
    ]);
    fixture
        .anchr()
        .args(["rename", "auth/flow", "auth/token-refresh", "--dry-run"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("would edit docs/a.md (2 sites)"))
        .stdout(predicate::str::contains("would edit src/x.rs (1 sites)"));
    assert!(fixture.read("docs/a.md").contains("@anchor[auth/flow]"));

    fixture
        .anchr()
        .args(["rename", "auth/flow", "auth/token-refresh"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("edited docs/a.md"))
        .stdout(predicate::str::contains("edited src/x.rs"))
        .stdout(predicate::str::contains("1 declaration, 2 references"));
    assert_eq!(
        fixture.read("docs/a.md"),
        "# Flow @anchor[auth/token-refresh]\n\nSee @ref[#auth/token-refresh].\n"
    );
    assert_eq!(
        fixture.read("src/x.rs"),
        "// @ref[#auth/token-refresh]\nfn f() {}\n"
    );

    fixture
        .anchr()
        .args(["check", "--color", "never"])
        .assert()
        .code(0);
}

#[test]
fn rename_refuses_unknown_or_colliding_ids() {
    let fixture = Fixture::new(&[("a.md", "@anchor[a] @anchor[b]")]);
    fixture
        .anchr()
        .args(["rename", "zzz", "q"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no anchor `zzz`"));
    fixture
        .anchr()
        .args(["rename", "a", "b"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("already exists"));
    fixture
        .anchr()
        .args(["rename", "a", "not valid"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a valid anchor id"));
    assert_eq!(fixture.read("a.md"), "@anchor[a] @anchor[b]");
}
