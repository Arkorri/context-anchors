use std::fs;

use camino::Utf8PathBuf;

use super::*;
use crate::check::{CheckOptions, check};
use crate::config;

struct Fixture {
    _dir: tempfile::TempDir,
    root_dir: Utf8PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = Utf8PathBuf::from_path_buf(dir.path().join("repo")).unwrap();
        for (path, contents) in files {
            let full = root_dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        Self {
            _dir: dir,
            root_dir,
        }
    }

    fn workspace(&self) -> Workspace {
        Workspace::load(config::discover(&self.root_dir).unwrap()).unwrap()
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root_dir.join(path)).unwrap()
    }
}

fn id(raw: &str) -> AnchorId {
    AnchorId::parse(raw).unwrap()
}

#[test]
fn an_alias_rename_stays_in_its_file_and_covers_every_declaration_and_use() {
    let fixture = Fixture::new(&[
        (
            "a.md",
            "@anchor[x]\n@ref[#x as O] @[O] @[O] @ref[#x as O] @ref[#x as Other]\n",
        ),
        ("b.md", "@ref[#x as O] @[O]\n"),
    ]);
    let workspace = fixture.workspace();
    let path = FilePath::new(Utf8PathBuf::from("a.md")).unwrap();
    let alias = |raw: &str| Alias::parse(raw).unwrap();

    let plan = plan_alias_rename(&workspace, &path, &alias("O"), &alias("Old")).unwrap();
    assert_eq!((plan.declaration_sites, plan.use_sites), (2, 2));
    let rewritten = crate::edit::apply_edits(&fixture.read("a.md"), &plan.edits).unwrap();
    assert_eq!(
        rewritten,
        "@anchor[x]\n@ref[#x as Old] @[Old] @[Old] @ref[#x as Old] @ref[#x as Other]\n"
    );

    assert!(matches!(
        plan_alias_rename(&workspace, &path, &alias("O"), &alias("Other")),
        Err(AliasRenameError::TargetExists { .. })
    ));
    assert!(matches!(
        plan_alias_rename(&workspace, &path, &alias("Nope"), &alias("X")),
        Err(AliasRenameError::Unknown { .. })
    ));
    assert!(matches!(
        plan_alias_rename(&workspace, &path, &alias("O"), &alias("O")),
        Err(AliasRenameError::SameAlias)
    ));
}

#[test]
fn an_aliased_declaration_is_rewritten_and_its_uses_are_left_alone() {
    let fixture = Fixture::new(&[("a.md", "@anchor[old]\n@ref[#old as O] @[O] @[O]\n")]);
    let plan = plan_rename(&fixture.workspace(), &id("old"), &id("new")).unwrap();
    assert_eq!((plan.anchor_sites, plan.ref_sites), (1, 1));
    apply_rename(&plan, &fixture.root_dir).unwrap();
    assert_eq!(
        fixture.read("a.md"),
        "@anchor[new]\n@ref[#new as O] @[O] @[O]\n"
    );
}

#[test]
fn a_rename_rewrites_only_id_bytes_across_prose_and_comments() {
    let fixture = Fixture::new(&[
        ("anchr.toml", "[roots]\nplugin = \"../not-there\"\n"),
        (
            "docs/a.md",
            "# Flow @anchor[auth/flow]\n\nSee @ref[#auth/flow] and @ref[repo:#auth/flow].\n\n```\n@ref[#auth/flow] stays: it is an example\n```\n",
        ),
        (
            "src/x.rs",
            "// @ref[#auth/flow] and `@ref[#auth/flow]` (example)\nfn f() {}\n",
        ),
        (
            "docs/other.md",
            "@ref[#unrelated] @anchor[unrelated] @ref[plugin:#auth/flow]\n",
        ),
    ]);
    let workspace = fixture.workspace();
    let plan = plan_rename(&workspace, &id("auth/flow"), &id("auth/token-refresh")).unwrap();
    assert_eq!(plan.anchor_sites, 1);
    assert_eq!(plan.ref_sites, 3);
    assert_eq!(plan.edit_count(), 4);
    assert_eq!(
        plan.files().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["docs/a.md", "src/x.rs"]
    );

    let written = apply_rename(&plan, &fixture.root_dir).unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(
        fixture.read("docs/a.md"),
        "# Flow @anchor[auth/token-refresh]\n\nSee @ref[#auth/token-refresh] and @ref[repo:#auth/token-refresh].\n\n```\n@ref[#auth/flow] stays: it is an example\n```\n"
    );
    assert_eq!(
        fixture.read("src/x.rs"),
        "// @ref[#auth/token-refresh] and `@ref[#auth/flow]` (example)\nfn f() {}\n"
    );
    assert!(fixture.read("docs/other.md").contains("plugin:#auth/flow"));

    let report = check(&fixture.workspace(), &CheckOptions::default()).unwrap();
    let errors: Vec<String> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostic::Severity::Error)
        .map(|d| d.kind.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn planning_rejects_unknown_existing_and_identical_ids() {
    let fixture = Fixture::new(&[("a.md", "@anchor[a] @anchor[b]")]);
    let workspace = fixture.workspace();
    assert!(matches!(
        plan_rename(&workspace, &id("zzz"), &id("q")),
        Err(RenameError::UnknownAnchor(_))
    ));
    assert!(matches!(
        plan_rename(&workspace, &id("a"), &id("b")),
        Err(RenameError::TargetExists(_))
    ));
    assert!(matches!(
        plan_rename(&workspace, &id("a"), &id("a")),
        Err(RenameError::SameId)
    ));
}

#[test]
fn applying_refuses_a_file_that_changed_since_the_scan() {
    let fixture = Fixture::new(&[("a.md", "@anchor[old] @ref[#old]")]);
    let workspace = fixture.workspace();
    let plan = plan_rename(&workspace, &id("old"), &id("new")).unwrap();
    fs::write(
        fixture.root_dir.join("a.md"),
        "moved @anchor[old] @ref[#old]",
    )
    .unwrap();
    assert!(matches!(
        apply_rename(&plan, &fixture.root_dir),
        Err(ApplyError::FileChanged { .. })
    ));
    assert_eq!(fixture.read("a.md"), "moved @anchor[old] @ref[#old]");
}
