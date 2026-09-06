//! `anchr coverage` and `anchr annotate` against fixture repositories.
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

fn fixture() -> Fixture {
    Fixture::new(&[
        (
            "README.md",
            "See `docs/guide.md` and docs/missing.md; call `run_check`. Already @ref[docs/guide.md].\n",
        ),
        ("docs/guide.md", "# Guide\n"),
        ("src/lib.rs", "pub fn run_check() {}\n"),
    ])
}

#[test]
fn coverage_reports_candidates_and_never_fails() {
    let fixture = fixture();
    fixture
        .anchr()
        .args(["coverage", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("README.md:1:5: `docs/guide.md` — could be @ref[docs/guide.md]"))
        .stdout(predicate::str::contains("docs/missing.md — does not resolve"))
        .stdout(predicate::str::contains("`run_check` — could be @ref[src/lib.rs#run_check]"))
        .stdout(predicate::str::contains(
            "1 of 4 reference-shaped strings are annotated; 2 could be, 1 do not resolve, 0 are ambiguous",
        ));

    let output = fixture
        .anchr()
        .args(["coverage", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["summary"]["total"], 4);
    assert_eq!(json["summary"]["proposals"], 2);
    let candidates = json["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    assert_eq!(candidates[0]["kind"], "proposal");
    assert_eq!(candidates[0]["location"]["region"], "inline-code");
    assert_eq!(candidates[1]["kind"], "unresolvable");
}

#[test]
fn annotate_only_writes_with_the_flag_and_the_result_passes_check() {
    let fixture = fixture();
    fixture
        .anchr()
        .args(["annotate", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains(
            "README.md:1:5: `docs/guide.md` -> @ref[docs/guide.md]",
        ))
        .stdout(predicate::str::contains(
            "2 proposals; pass --write to apply",
        ));
    assert!(fixture.read("README.md").contains("`docs/guide.md`"));

    fixture
        .anchr()
        .args(["annotate", "--write", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("edited README.md"))
        .stdout(predicate::str::contains("annotated 2 references"));
    assert_eq!(
        fixture.read("README.md"),
        "See @ref[docs/guide.md] and docs/missing.md; call @ref[src/lib.rs#run_check]. Already @ref[docs/guide.md].\n"
    );

    fixture
        .anchr()
        .args(["check", "--color", "never"])
        .assert()
        .code(0);

    fixture
        .anchr()
        .args(["coverage", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains(
            "3 of 4 reference-shaped strings are annotated",
        ));
}
