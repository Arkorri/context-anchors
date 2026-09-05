//! Renaming an anchor id: rewrite exactly the id bytes of every declaration and reference in
//! the current root. A convenience, never load-bearing; `check` remains the guarantee.

use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;

use crate::check::Workspace;
use crate::marker::{AnchorId, RefTarget};
use crate::root::FilePath;
use crate::span::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub span: ByteSpan,
    /// What the span currently holds, checked again before writing.
    pub expected: String,
    pub replacement: String,
}

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

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{path} changed since it was scanned; run the rename again")]
    FileChanged { path: FilePath },
    #[error("reading {path}: {source}")]
    Read {
        path: FilePath,
        #[source]
        source: std::io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: FilePath,
        #[source]
        source: std::io::Error,
    },
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
        // Anchor targets always carry an id span; `None` cannot occur here.
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
/// Returns the files written, in order.
pub fn apply_rename(plan: &RenamePlan, root_dir: &Utf8Path) -> Result<Vec<FilePath>, ApplyError> {
    let mut rewritten = Vec::with_capacity(plan.edits.len());
    for (path, edits) in &plan.edits {
        let absolute = root_dir.join(path.as_path());
        let source = fs::read_to_string(&absolute).map_err(|source| ApplyError::Read {
            path: path.clone(),
            source,
        })?;
        let updated = apply_edits(&source, edits)
            .ok_or_else(|| ApplyError::FileChanged { path: path.clone() })?;
        fs::write(&absolute, updated).map_err(|source| ApplyError::Write {
            path: path.clone(),
            source,
        })?;
        rewritten.push(path.clone());
    }
    Ok(rewritten)
}

/// `None` when any edit's expected text is not where the plan said it would be.
pub fn apply_edits(source: &str, edits: &[TextEdit]) -> Option<String> {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        let current = source.get(edit.span.start..edit.span.end)?;
        if current != edit.expected || edit.span.start < cursor {
            return None;
        }
        output.push_str(source.get(cursor..edit.span.start)?);
        output.push_str(&edit.replacement);
        cursor = edit.span.end;
    }
    output.push_str(source.get(cursor..)?);
    Some(output)
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

    #[test]
    fn edits_apply_back_to_front_safely_with_length_changes() {
        let source = "@anchor[ab] @ref[#ab] tail";
        let edits = vec![
            TextEdit {
                span: ByteSpan::new(8, 10),
                expected: "ab".into(),
                replacement: "longer-name".into(),
            },
            TextEdit {
                span: ByteSpan::new(18, 20),
                expected: "ab".into(),
                replacement: "longer-name".into(),
            },
        ];
        assert_eq!(
            apply_edits(source, &edits).unwrap(),
            "@anchor[longer-name] @ref[#longer-name] tail"
        );
    }
}
