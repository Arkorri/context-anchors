//! Renaming an anchor id: rewrite exactly the id bytes of every declaration and reference in
//! the current root. Renaming an alias: rewrite its declaration token and every use in the one
//! file that declares it. Conveniences, never load-bearing; `check` remains the guarantee.

use std::collections::BTreeMap;

use camino::Utf8Path;

use crate::check::Workspace;
use crate::edit::{ApplyError, TextEdit, apply_to_files};
use crate::marker::{Alias, AnchorId, MarkerPayload, RefTarget};
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

/// Edits renaming one file's alias: every declaration token for it and every `@[alias]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasRenamePlan {
    pub path: FilePath,
    pub old: Alias,
    pub new: Alias,
    /// Sorted by span start.
    pub edits: Vec<TextEdit>,
    pub declaration_sites: usize,
    pub use_sites: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum AliasRenameError {
    #[error("no alias `{alias}` is declared in `{path}`")]
    Unknown { path: FilePath, alias: Alias },
    #[error("an alias `{alias}` is already declared in `{path}`")]
    TargetExists { path: FilePath, alias: Alias },
    #[error("old and new aliases are the same")]
    SameAlias,
}

/// Plans an alias rename. Aliases are file-scoped, so the plan never leaves `path`; a duplicated
/// declaration is rewritten at every declaration site, since all of them spell the old name.
pub fn plan_alias_rename(
    workspace: &Workspace,
    path: &FilePath,
    old: &Alias,
    new: &Alias,
) -> Result<AliasRenamePlan, AliasRenameError> {
    if old == new {
        return Err(AliasRenameError::SameAlias);
    }
    let (_, index) = workspace.current();
    let record = index
        .file_record(path)
        .filter(|record| record.aliases.binding(old).is_some())
        .ok_or_else(|| AliasRenameError::Unknown {
            path: path.clone(),
            alias: old.clone(),
        })?;
    if record.aliases.binding(new).is_some() {
        return Err(AliasRenameError::TargetExists {
            path: path.clone(),
            alias: new.clone(),
        });
    }

    let mut edits = Vec::new();
    let mut declaration_sites = 0;
    let mut use_sites = 0;
    for marker in &record.markers {
        let span = match &marker.payload {
            MarkerPayload::Ref {
                alias: Some(declared),
                ..
            } if declared.alias == *old => {
                declaration_sites += 1;
                declared.span
            }
            MarkerPayload::Use { alias } if alias == old => {
                use_sites += 1;
                marker.body_span
            }
            MarkerPayload::Anchor { .. }
            | MarkerPayload::Ref { .. }
            | MarkerPayload::Use { .. }
            | MarkerPayload::NoRef { .. } => {
                continue;
            }
        };
        edits.push(TextEdit {
            span,
            expected: old.as_str().to_owned(),
            replacement: new.as_str().to_owned(),
        });
    }
    edits.sort_by_key(|edit| edit.span.start);
    Ok(AliasRenamePlan {
        path: path.clone(),
        old: old.clone(),
        new: new.clone(),
        edits,
        declaration_sites,
        use_sites,
    })
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
mod tests;
