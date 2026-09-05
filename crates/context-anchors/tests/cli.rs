//! End-to-end runs of the `anchr` binary against fixture repositories built in temp dirs.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

struct Fixture {
    dir: tempfile::TempDir,
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
        Self { dir }
    }

    fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("repo")
    }

    fn anchr(&self) -> Command {
        let mut command = Command::cargo_bin("anchr").unwrap();
        command.current_dir(self.root()).env_remove("NO_COLOR");
        command
    }

    fn check_json(&self, extra: &[&str]) -> (i32, serde_json::Value) {
        let output = self
            .anchr()
            .args(["check", "--format", "json"])
            .args(extra)
            .output()
            .unwrap();
        let code = output.status.code().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "invalid json ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (code, json)
    }
}

/// Temp paths differ per run, may be reached through a symlink (macOS `/var` →
/// `/private/var`), and appear with doubled backslashes inside JSON strings on Windows;
/// replace every spelling so snapshots are stable across platforms. Paths joined below the
/// root keep the platform separator, which is normalized to `/` after the placeholder.
fn scrub(text: &str, root: &Path) -> String {
    let canonical = root.canonicalize().unwrap();
    [canonical.as_path(), root]
        .into_iter()
        .flat_map(|path| {
            let plain = path.to_str().unwrap().to_owned();
            let json_escaped = serde_json::to_string(&plain).unwrap();
            [plain, json_escaped.trim_matches('"').to_owned()]
        })
        .fold(text.to_owned(), |text, spelling| {
            text.replace(&spelling, "[ROOT]")
        })
        .replace("[ROOT]\\\\", "[ROOT]/")
        .replace("[ROOT]\\", "[ROOT]/")
}

#[test]
fn a_clean_repo_exits_zero_with_a_summary() {
    let fixture = Fixture::new(&[
        (
            "README.md",
            "# Repo @anchor[readme]\n\nSee @ref[#readme] and @ref[src/lib.rs#run].\n",
        ),
        ("src/lib.rs", "// @ref[#readme]\npub fn run() {}\n"),
    ]);
    fixture
        .anchr()
        .args(["check", "--color", "never"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains(
            "checked 3 references in 2 files (1 anchors): 3 resolved, 0 errors, 0 unverified",
        ));
}

#[test]
fn broken_references_exit_one_and_render_grouped_by_cause() {
    let fixture = Fixture::new(&[
        (
            "docs/a.md",
            "# A\n\nSee @ref[#auth/flow] and again @ref[#auth/flow].\n",
        ),
        (
            "docs/b.md",
            "@anchor[auth/token-refresh]\n\n@ref[#auth/flow]\n",
        ),
        ("src/x.rs", "// @ref[#auth/flow]\nfn f() {}\n"),
    ]);
    let output = fixture
        .anchr()
        .args(["check", "--color", "never"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = scrub(&String::from_utf8(output.stdout).unwrap(), &fixture.root());
    insta::assert_snapshot!("human_grouped_by_cause", stdout);
}

#[test]
fn json_output_has_a_stable_schema() {
    let fixture = Fixture::new(&[
        ("anchr.toml", "[roots]\nclaude = \"../not-there\"\n"),
        (
            "docs/a.md",
            "@anchor[dup] @ref[#missing] @ref[claude:#x] @ref[\n",
        ),
        ("docs/b.md", "@anchor[dup] @ref[docs/a.md#Foo::bar]\n"),
        ("src/thing.ex", "defmodule X do end\n"),
        ("src/y.rs", "// @ref[src/thing.ex#anything]\n"),
    ]);
    let (code, json) = fixture.check_json(&[]);
    assert_eq!(code, 1);
    let scrubbed: serde_json::Value =
        serde_json::from_str(&scrub(&json.to_string(), &fixture.root())).unwrap();
    insta::assert_json_snapshot!("json_report", scrubbed);
}

#[test]
fn strict_promotes_unverified_findings_to_errors() {
    let fixture = Fixture::new(&[
        ("anchr.toml", "[roots]\nclaude = \"../not-there\"\n"),
        ("a.md", "@ref[claude:#x]\n"),
    ]);
    let (code, json) = fixture.check_json(&[]);
    assert_eq!(code, 0);
    assert_eq!(json["summary"]["unverified"], 1);
    assert_eq!(json["summary"]["strict"], false);

    let (code, json) = fixture.check_json(&["--strict"]);
    assert_eq!(code, 1);
    assert_eq!(json["summary"]["errors"], 1);
    assert_eq!(json["summary"]["strict"], true);
    assert_eq!(json["diagnostics"][0]["code"], "root-absent");
    assert_eq!(json["diagnostics"][0]["severity"], "error");
}

#[test]
fn invalid_config_exits_two_with_the_offending_key() {
    let fixture = Fixture::new(&[("anchr.toml", "[scan]\nincldue = []\n"), ("a.md", "")]);
    fixture
        .anchr()
        .args(["check"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("incldue"))
        .stderr(predicate::str::contains("anchr.toml"));
}

#[test]
fn a_semantic_config_error_names_the_field() {
    let fixture = Fixture::new(&[("anchr.toml", "[scan]\nmax-file-bytes = 0\n"), ("a.md", "")]);
    fixture
        .anchr()
        .args(["check"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("scan.max-file-bytes"));
}

#[test]
fn paths_filter_references_but_not_root_wide_findings() {
    let fixture = Fixture::new(&[
        ("a.md", "@ref[#gone] @anchor[dup]\n"),
        ("b.md", "@ref[#gone] @anchor[dup]\n"),
    ]);
    let (code, json) = fixture.check_json(&["a.md"]);
    assert_eq!(code, 1);
    assert_eq!(json["summary"]["refs_checked"], 1);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    let missing = diagnostics
        .iter()
        .find(|d| d["code"] == "anchor-missing")
        .unwrap();
    assert_eq!(missing["locations"].as_array().unwrap().len(), 1);
    let duplicate = diagnostics
        .iter()
        .find(|d| d["code"] == "duplicate-anchor")
        .unwrap();
    assert_eq!(duplicate["locations"].as_array().unwrap().len(), 2);
}

#[test]
fn a_path_outside_the_root_is_a_usage_error() {
    let fixture = Fixture::new(&[("a.md", "")]);
    fixture
        .anchr()
        .args(["check", "../elsewhere.md"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("outside the root"));
}

#[test]
fn root_flag_starts_discovery_elsewhere() {
    let fixture = Fixture::new(&[("nested/deep/a.md", "@ref[#nope]\n")]);
    let root = fixture.root();
    let mut command = Command::cargo_bin("anchr").unwrap();
    command.current_dir(fixture.dir.path());
    let (stdout, code) = {
        let output = command
            .args(["check", "--format", "json", "--root"])
            .arg(root.join("nested/deep"))
            .output()
            .unwrap();
        (
            String::from_utf8(output.stdout).unwrap(),
            output.status.code().unwrap(),
        )
    };
    assert_eq!(code, 1);
    assert!(stdout.contains("anchor-missing"));
}

#[test]
fn completions_are_generated_for_a_shell() {
    Command::cargo_bin("anchr")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("anchr"));
}
