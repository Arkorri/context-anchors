//! Renaming an anchor id: rewrite exactly the id bytes of every declaration and reference in
//! the current root. A convenience, never load-bearing; `check` remains the guarantee.

use std::collections::BTreeMap;

use camino::Utf8Path;

use crate::check::Workspace;
use crate::edit::{ApplyError, TextEdit, apply_to_files};
use crate::marker::{AnchorId, RefTarget};
use crate::root::FilePath;
use crate::span::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    pub old: AnchorId,
    pub new: AnchorId,
    /// Edits per file, sorted by span start.
    pub edits: BTreeMap<FilePath, Vec<TextEdit>>,
    pub anchor_sites: usize,
    pub ref_sites: usize,
}

impl RenamePlan {
    pub fn files(&self) -> impl Iterator<Item = &FilePath> {
        self.edits.keys()
    }

    pub fn edit_count(&self) -> usize {
        self.edits.values().map(Vec::len).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("no anchor `{0}` in the current root")]
    UnknownAnchor(AnchorId),
    #[error("an anchor `{0}` already exists in the current root")]
    TargetExists(AnchorId),
    #[error("old and new ids are the same")]
    SameId,
}

/// Plans a rename within the current root. References from external roots to this anchor
/// cannot be seen from here and are not touched; `check` in those roots will report them.
pub fn plan_rename(
    workspace: &Workspace,
    old: &AnchorId,
    new: &AnchorId,
) -> Result<RenamePlan, RenameError> {
    if old == new {
        return Err(RenameError::SameId);
    }
    let (root, index) = workspace.current();
    if index.anchor_sites(old).is_empty() {
        return Err(RenameError::UnknownAnchor(old.clone()));
    }
    if !index.anchor_sites(new).is_empty() {
        return Err(RenameError::TargetExists(new.clone()));
    }

    let mut edits: BTreeMap<FilePath, Vec<TextEdit>> = BTreeMap::new();
    let mut anchor_sites = 0;
    let mut ref_sites = 0;

    for anchor in index.anchors().filter(|anchor| anchor.id == old) {
        anchor_sites += 1;
        edits
            .entry(anchor.site.path)
            .or_default()
            .push(edit(anchor.id_span, old, new));
    }

    let target = RefTarget::Anchor {
        root: Some(root.name.clone()),
        id: old.clone(),
    };
    for reference in index.backrefs(&target) {
        // A use reaches the anchor through its declaration, which is rewritten as a direct ref.
        if reference.via.is_some() {
            continue;
        }
        // Direct anchor targets always carry an id span; `None` cannot occur here.
        let Some(id_span) = reference.id_span else {
            continue;
        };
        ref_sites += 1;
        edits
            .entry(reference.site.path)
            .or_default()
            .push(edit(id_span, old, new));
    }

    for file_edits in edits.values_mut() {
        file_edits.sort_by_key(|edit| edit.span.start);
    }
    Ok(RenamePlan {
        old: old.clone(),
        new: new.clone(),
        edits,
        anchor_sites,
        ref_sites,
    })
}

/// Applies a plan, refusing any file whose bytes no longer match what was planned.
pub fn apply_rename(plan: &RenamePlan, root_dir: &Utf8Path) -> Result<Vec<FilePath>, ApplyError> {
    apply_to_files(root_dir, &plan.edits)
}

fn edit(span: ByteSpan, old: &AnchorId, new: &AnchorId) -> TextEdit {
    TextEdit {
        span,
        expected: old.as_str().to_owned(),
        replacement: new.as_str().to_owned(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
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
}
