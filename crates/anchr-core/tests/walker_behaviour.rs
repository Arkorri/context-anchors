//! Spike tests pinning third-party behaviour the scan design depends on (@ref[#code/scan],
//! @ref[#code/security]). If one of these fails after a dependency bump, the scan stage's
//! assumptions changed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::sync::mpsc;

use ignore::overrides::OverrideBuilder;
use ignore::{WalkBuilder, WalkState};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn walked_file_names(builder: &WalkBuilder) -> Vec<String> {
    let (sender, receiver) = mpsc::channel();
    builder.build_parallel().run(|| {
        let sender = sender.clone();
        Box::new(move |entry| {
            if let Ok(entry) = entry
                && entry.file_type().is_some_and(|kind| kind.is_file())
            {
                sender
                    .send(entry.file_name().to_string_lossy().into_owned())
                    .unwrap();
            }
            WalkState::Continue
        })
    });
    drop(sender);
    let mut names: Vec<String> = receiver.iter().collect();
    names.sort();
    names
}

fn base_walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .follow_links(false);
    builder
}

#[test]
fn gitignore_is_honoured_without_a_git_directory() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(".gitignore"), "generated.md\n");
    write(&dir.path().join("kept.md"), "");
    write(&dir.path().join("generated.md"), "");

    let names = walked_file_names(&base_walker(dir.path()));

    assert_eq!(names, vec!["kept.md"]);
}

#[test]
fn whitelist_override_bypasses_gitignore_so_include_must_be_a_post_filter() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(".gitignore"), "generated.md\n");
    write(&dir.path().join("kept.md"), "");
    write(&dir.path().join("generated.md"), "");

    let mut overrides = OverrideBuilder::new(dir.path());
    overrides.add("*.md").unwrap();
    let mut builder = base_walker(dir.path());
    builder.overrides(overrides.build().unwrap());

    let names = walked_file_names(&builder);

    assert!(
        names.contains(&"generated.md".to_owned()),
        "if this starts failing, `ignore` changed override precedence and include globs could \
         move back into overrides; see CODE_DESIGN.md §3.3"
    );
}

#[test]
fn exclude_override_removes_files_the_gitignore_kept() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("kept.md"), "");
    write(&dir.path().join("vendor/third_party.md"), "");

    let mut overrides = OverrideBuilder::new(dir.path());
    overrides.add("!vendor/**").unwrap();
    let mut builder = base_walker(dir.path());
    builder.overrides(overrides.build().unwrap());

    let names = walked_file_names(&builder);

    assert_eq!(names, vec!["kept.md"]);
}

#[test]
fn symlinks_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("secret.md"), "");
    write(&dir.path().join("kept.md"), "");
    symlink_dir(outside.path(), &dir.path().join("linked"));

    let names = walked_file_names(&base_walker(dir.path()));

    assert_eq!(names, vec!["kept.md"]);
}

#[test]
fn a_panic_in_a_visitor_propagates_to_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("kept.md"), "");

    let outcome = std::panic::catch_unwind(|| {
        base_walker(dir.path()).build_parallel().run(|| {
            Box::new(|entry| {
                if entry.is_ok_and(|entry| entry.file_type().is_some_and(|kind| kind.is_file())) {
                    panic!("visitor panic");
                }
                WalkState::Continue
            })
        });
    });

    assert!(
        outcome.is_err(),
        "a visitor panic was swallowed; the scan stage needs an explicit join-and-resume"
    );
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}
