//! The fixture every integration module builds on. One constructor and no options: it always
//! builds the most realistic repository and the most isolated command. A test that needs an
//! unusual layout builds it through `root()` inside the test; a helper is promoted here only when
//! a second module wants it; the constructor never grows a parameter.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

pub struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    /// Writes `files` under `<tmp>/repo` plus a `.git` marker, because `.gitignore` files carry
    /// weight only inside a repository. `<tmp>/home` is the child process's home (see `anchr`).
    pub fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        for (path, contents) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        Self { dir, root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `anchr` run in the repository with the developer's shell kept out. `NO_COLOR` is removed
    /// so colour assertions see the flags alone. The home and git variables point at the empty
    /// `<tmp>/home`, which covers every place the `ignore` crate looks for a global excludes file
    /// (`GIT_CONFIG_GLOBAL`, `~/.gitconfig`, `$XDG_CONFIG_HOME/git/config`, the system config,
    /// `~/.config/git/ignore`); the binary itself reads a home only to expand `~` in config.
    pub fn anchr(&self) -> Command {
        let home = self.dir.path().join("home");
        let gitconfig = home.join("gitconfig");
        let mut command = Command::cargo_bin("anchr").unwrap();
        command
            .current_dir(&self.root)
            .env_remove("NO_COLOR")
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", &home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig);
        command
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
    }

    /// `check --format json` plus `extra`, parsed, with the raw output in the failure message.
    pub fn check_json(&self, extra: &[&str]) -> (i32, serde_json::Value) {
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
