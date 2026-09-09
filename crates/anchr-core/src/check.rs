//! Loading a workspace (scan every present root, index them) and the one guarantee built on
//! it: `check`. Broken references are data in the report; only tool failures are errors.

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;

use crate::config::{ConfigError, Discovered, UnverifiedPolicy};
use crate::diagnostic::{
    DiagnosticKind, FileLocation, LocatedSite, Report, ReportBuilder, Summary,
};
use crate::index::{Index, Site};
use crate::marker::Alias;
use crate::resolve::{IndexedRoot, IndexedRoots, Resolution, Resolver};
use crate::root::{FilePath, RootName, RootSet, RootSetError};
use crate::scan::{ScanMode, SkippedFile, WalkProblem, scan_root};
use crate::span::PositionOverflow;
use crate::suggest::suggest;
use crate::text::{AnalyzeError, Container, FileAnalyzer, LanguageRegistry, RegistryError};

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Overrides `[check] unverified` from config when set.
    pub unverified: Option<UnverifiedPolicy>,
    /// When non-empty, only references and malformed markers in these files are reported.
    /// Root-wide findings (duplicates, skipped files) are always reported.
    pub only_files: Vec<FilePath>,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Roots(#[from] RootSetError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("a marker offset exceeded the addressable range: {0}")]
    Position(#[from] PositionOverflow),
}

/// What a root's scan learned beyond its index: files it could not check, kept for the report,
/// and the extensions it walked past, which coverage's path rule reads.
#[derive(Debug, Clone, Default)]
pub struct ScanFindings {
    pub skipped: Vec<SkippedFile>,
    pub problems: Vec<WalkProblem>,
    pub extensions: BTreeSet<String>,
}

/// Every present root scanned and indexed, ready for checking, querying, or renaming.
pub struct Workspace {
    pub registry: LanguageRegistry,
    pub roots: IndexedRoots,
    pub policy: UnverifiedPolicy,
    pub findings: BTreeMap<RootName, ScanFindings>,
    current_files_scanned: usize,
}

impl Workspace {
    /// The current root is scanned in full; external roots contribute anchors only.
    pub fn load(discovered: Discovered) -> Result<Self, CheckError> {
        let registry = LanguageRegistry::new()?;
        let policy = discovered.config.check.unverified;
        let root_set = RootSet::load(discovered.root_dir, discovered.config)?;

        let mut indexes = BTreeMap::new();
        let mut findings = BTreeMap::new();
        let mut current_files_scanned = 0usize;
        for root in root_set.present() {
            let is_current = root.name == *root_set.current_name();
            let mode = if is_current {
                ScanMode::Full
            } else {
                ScanMode::AnchorsOnly
            };
            let output = scan_root(root, &registry, mode);
            let crate::scan::ScanOutput {
                files,
                skipped,
                problems,
                extensions,
                tree,
            } = output;
            if is_current {
                current_files_scanned = files.len();
            }
            findings.insert(
                root.name.clone(),
                ScanFindings {
                    skipped,
                    problems,
                    extensions,
                },
            );
            indexes.insert(
                root.name.clone(),
                Index::from_scan(root.name.clone(), files, tree),
            );
        }

        Ok(Self {
            roots: IndexedRoots::new(&root_set, indexes),
            registry,
            policy,
            findings,
            current_files_scanned,
        })
    }

    pub fn current(&self) -> (&crate::root::Root, &Index) {
        self.roots.current()
    }

    /// Re-lexes one file of the current root from `text` (an editor buffer, not the disk) and
    /// replaces its index entry. Returns `false` when the file has no container and is
    /// therefore not scanned.
    pub fn update_file(&mut self, path: FilePath, text: &str) -> Result<bool, AnalyzeError> {
        let (root, index) = self.roots.current_mut();
        let Some(container) =
            Container::for_path(path.as_path(), &root.config.containers, &self.registry)
        else {
            index.mark_present(&path);
            index.remove_file(&path);
            return Ok(false);
        };
        let mut analyzer = FileAnalyzer::new(&self.registry, root.config.scan.parse_budget);
        let scan = analyzer.scan(container, text, &path)?;
        index.update_file(path, scan);
        Ok(true)
    }

    /// Re-lexes one file of the current root from disk, or drops it if it is gone.
    pub fn reload_file(&mut self, path: FilePath) -> Result<bool, AnalyzeError> {
        let (root, _) = self.roots.current();
        match std::fs::read_to_string(root.dir.join(path.as_path())) {
            Ok(text) => self.update_file(path, &text),
            Err(_) => {
                let (_, index) = self.roots.current_mut();
                index.remove_file(&path);
                index.mark_absent(&path);
                Ok(false)
            }
        }
    }

    pub fn root_dirs(&self) -> BTreeMap<RootName, Utf8PathBuf> {
        self.roots
            .present()
            .map(|(root, _)| (root.name.clone(), root.dir.clone()))
            .collect()
    }
}

pub fn run_check(discovered: Discovered, options: &CheckOptions) -> Result<Report, CheckError> {
    let workspace = Workspace::load(discovered)?;
    check(&workspace, options)
}

pub fn check(workspace: &Workspace, options: &CheckOptions) -> Result<Report, CheckError> {
    let policy = options.unverified.unwrap_or(workspace.policy);
    let (current_root, current_index) = workspace.current();

    let mut builder = ReportBuilder::default();
    let mut summary = Summary {
        roots_scanned: workspace.roots.present().count(),
        files_scanned: workspace.current_files_scanned,
        anchors: current_index.anchor_count(),
        ..Summary::default()
    };
    for (root, findings) in &workspace.findings {
        record_scan_findings(&mut builder, root, findings);
    }

    let mut resolver = Resolver::new(&workspace.roots, &workspace.registry);
    let in_scope =
        |site: &Site| options.only_files.is_empty() || options.only_files.contains(&site.path);

    for reference in current_index.refs() {
        if !in_scope(&reference.site) {
            continue;
        }
        summary.refs_checked += 1;
        let path = reference.site.path.clone();
        let located = locate(current_index, reference.site)?;
        let kind = match resolver.resolve(&current_root.name, reference.target) {
            Resolution::Resolved => {
                summary.refs_resolved += 1;
                continue;
            }
            Resolution::Unresolved(unresolved) => {
                let suggestion = resolver.suggest(&unresolved);
                let explanation = resolver.explain(&unresolved);
                let relative = resolver.relative_note(&unresolved, &path);
                let kind = DiagnosticKind::Unresolved(unresolved);
                builder.suggestion(&kind, suggestion);
                for note in [explanation, relative].into_iter().flatten() {
                    builder.note(&kind, note);
                }
                kind
            }
            Resolution::Unverified(unverified) => DiagnosticKind::Unverified(unverified),
        };
        if let Some(alias) = reference.declares {
            note_alias_uses(&mut builder, &kind, current_index, &path, alias);
        }
        builder.site(kind, located);
    }

    for use_site in current_index.alias_uses() {
        if !in_scope(&use_site.site) {
            continue;
        }
        if use_site.binding.is_some() {
            summary.alias_uses += 1;
            continue;
        }
        let path = use_site.site.path.clone();
        let kind = DiagnosticKind::AliasUndeclared {
            root: current_root.name.clone(),
            path: path.clone(),
            alias: use_site.alias.clone(),
        };
        let declared: Vec<&str> = current_index
            .file_record(&path)
            .map(|record| record.aliases.aliases().map(Alias::as_str).collect())
            .unwrap_or_default();
        builder.suggestion(&kind, suggest(use_site.alias.as_str(), declared));
        builder.site(kind, locate(current_index, use_site.site)?);
    }

    for (path, alias, sites) in current_index.duplicate_aliases() {
        if !options.only_files.is_empty() && !options.only_files.contains(path) {
            continue;
        }
        let kind = DiagnosticKind::AliasDuplicate {
            root: current_root.name.clone(),
            path: path.clone(),
            alias: alias.clone(),
        };
        for site in sites {
            builder.site(kind.clone(), locate(current_index, site)?);
        }
    }

    for malformed in current_index.malformed() {
        if !in_scope(&malformed.site) {
            continue;
        }
        let located = locate(current_index, malformed.site)?;
        builder.site(
            DiagnosticKind::Malformed {
                kind: malformed.malformed.kind,
                reason: malformed.malformed.reason.clone(),
            },
            located,
        );
    }

    for (id, sites) in current_index.duplicate_anchors() {
        let kind = DiagnosticKind::DuplicateAnchor {
            root: current_root.name.clone(),
            id: id.clone(),
        };
        for site in sites {
            let located = locate(current_index, site)?;
            builder.site(kind.clone(), located);
        }
    }

    for (name, entry) in workspace.roots.external() {
        let IndexedRoot::Present { index, .. } = entry else {
            continue;
        };
        for (id, sites) in index.duplicate_anchors() {
            let kind = DiagnosticKind::ExternalDuplicate {
                root: name.clone(),
                id: id.clone(),
            };
            for site in sites {
                let located = locate(index, site)?;
                builder.site(kind.clone(), located);
            }
        }
    }

    Ok(builder.finish(policy, summary, workspace.root_dirs()))
}

/// A broken declaration is reported once, at the declaration; the note keeps its uses visible.
fn note_alias_uses(
    builder: &mut ReportBuilder,
    kind: &DiagnosticKind,
    index: &Index,
    path: &FilePath,
    alias: &Alias,
) {
    let uses = index.alias_use_count(path, alias);
    if uses > 0 {
        let plural = if uses == 1 { "" } else { "s" };
        builder.note(
            kind,
            format!("alias `{alias}` has {uses} use{plural} in `{path}`"),
        );
    }
}

fn record_scan_findings(builder: &mut ReportBuilder, root: &RootName, findings: &ScanFindings) {
    for skipped in &findings.skipped {
        builder.file(
            DiagnosticKind::FileSkipped {
                root: root.clone(),
                reason: skipped.reason.clone(),
            },
            FileLocation {
                root: root.clone(),
                path: skipped.path.clone(),
            },
        );
    }
    for problem in &findings.problems {
        let message = match &problem.path {
            Some(path) => format!("{path}: {}", problem.message),
            None => problem.message.clone(),
        };
        builder.root(
            DiagnosticKind::WalkProblem {
                root: root.clone(),
                message,
            },
            root.clone(),
        );
    }
}

/// Resolves a site's line and column from its index. The site's file is always present in the
/// index it came from; the `None` arm is unreachable in practice and reported, not swallowed.
pub fn locate(index: &Index, site: Site) -> Result<LocatedSite, PositionOverflow> {
    let line_col = match index.line_index(&site.path) {
        Some(line_index) => line_index.line_col(site.span.start)?,
        None => {
            return Err(PositionOverflow {
                offset: site.span.start,
            });
        }
    };
    Ok(LocatedSite { site, line_col })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
