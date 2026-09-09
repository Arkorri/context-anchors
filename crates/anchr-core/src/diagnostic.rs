//! Findings grouped by cause. Deleting an anchor with twelve live references is one
//! diagnostic with twelve locations, not twelve diagnostics.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use camino::Utf8PathBuf;

use crate::config::UnverifiedPolicy;
use crate::index::Site;
use crate::marker::{Alias, AnchorId, MalformedReason, MarkerKind};
use crate::resolve::{Unresolved, Unverified};
use crate::root::{FilePath, RootName};
use crate::scan::SkipReason;
use crate::span::LineCol;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Unverified,
}

/// The cause of a finding. Never contains a location, so it doubles as the grouping key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    Unresolved(Unresolved),
    DuplicateAnchor {
        root: RootName,
        id: AnchorId,
    },
    /// `@[alias]` in a file that never declares it. Keyed by file, because the file is the scope.
    AliasUndeclared {
        root: RootName,
        path: FilePath,
        alias: Alias,
    },
    AliasDuplicate {
        root: RootName,
        path: FilePath,
        alias: Alias,
    },
    Malformed {
        kind: MarkerKind,
        reason: MalformedReason,
    },
    Unverified(Unverified),
    /// A duplicate anchor in an external root: real, but not fixable from here.
    ExternalDuplicate {
        root: RootName,
        id: AnchorId,
    },
    FileSkipped {
        root: RootName,
        reason: SkipReason,
    },
    WalkProblem {
        root: RootName,
        message: String,
    },
}

impl DiagnosticKind {
    /// Intrinsic severity; `--strict` promotes
    /// @ref[./resolve/mod.rs#Unverified] at report time.
    pub fn base_severity(&self) -> Severity {
        match self {
            DiagnosticKind::Unresolved(_)
            | DiagnosticKind::DuplicateAnchor { .. }
            | DiagnosticKind::AliasUndeclared { .. }
            | DiagnosticKind::AliasDuplicate { .. }
            | DiagnosticKind::Malformed { .. } => Severity::Error,
            DiagnosticKind::Unverified(_)
            | DiagnosticKind::ExternalDuplicate { .. }
            | DiagnosticKind::FileSkipped { .. }
            | DiagnosticKind::WalkProblem { .. } => Severity::Unverified,
        }
    }

    /// For unverified findings, what would make them checkable.
    pub fn hint(&self) -> Option<String> {
        match self {
            DiagnosticKind::Unverified(Unverified::RootAbsent { name, declared_dir }) => Some(
                format!("root `{name}` is declared at {declared_dir}; create it, or fix `[roots] {name}` in anchr.toml"),
            ),
            DiagnosticKind::Unverified(Unverified::NoGrammar { extension, .. }) => Some(match extension {
                Some(ext) => format!("no bundled grammar for `.{ext}`; symbol references into these files cannot be checked"),
                None => "the file has no extension, so no grammar can be chosen".to_owned(),
            }),
            DiagnosticKind::Unverified(Unverified::ParseErrors { language, .. }) => Some(format!(
                "the `{language}` grammar could not parse the whole file; the declaration may sit inside a syntax error or use syntax newer than the bundled grammar"
            )),
            DiagnosticKind::Unverified(Unverified::ParseTimeout { .. }) => {
                Some("raise `scan.parse-budget-ms` in anchr.toml".to_owned())
            }
            DiagnosticKind::Unverified(Unverified::TargetTooLarge { .. })
            | DiagnosticKind::FileSkipped {
                reason: SkipReason::TooLarge { .. },
                ..
            } => Some("raise `scan.max-file-bytes` in anchr.toml; anchors in skipped files are not indexed".to_owned()),
            DiagnosticKind::FileSkipped {
                reason: SkipReason::NotUtf8 | SkipReason::Analyze(_),
                ..
            } => Some("anchors in skipped files are not indexed".to_owned()),
            DiagnosticKind::ExternalDuplicate { root, .. } => {
                Some(format!("fix the duplicate in root `{root}`; references to it still resolve"))
            }
            DiagnosticKind::AliasUndeclared { alias, .. } => Some(format!(
                "declare it once in this file: `@ref[target as {alias}]`"
            )),
            _ => None,
        }
    }
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticKind::Unresolved(unresolved) => match unresolved {
                Unresolved::PathMissing { root, path } => {
                    write!(f, "missing path `{path}` in root `{root}`")
                }
                Unresolved::PathNotDirectory { root, path } => {
                    write!(f, "`{path}` in root `{root}` is not a directory")
                }
                Unresolved::PathNotFile { root, path } => {
                    write!(f, "`{path}` in root `{root}` is a directory, not a file")
                }
                Unresolved::PathEscapesRoot { root, path } => {
                    write!(f, "`{path}` resolves outside root `{root}`; not read")
                }
                Unresolved::SymbolMissing { root, path, name } => {
                    write!(
                        f,
                        "no declaration named `{name}` in `{path}` (root `{root}`)"
                    )
                }
                Unresolved::AnchorMissing { root, id } => {
                    write!(f, "unknown anchor id `{id}` in root `{root}`")
                }
                Unresolved::RootUndeclared { name } => write!(f, "undeclared root `{name}`"),
            },
            DiagnosticKind::DuplicateAnchor { root, id } => {
                write!(
                    f,
                    "anchor id `{id}` is declared more than once in root `{root}`"
                )
            }
            DiagnosticKind::AliasUndeclared { root, path, alias } => {
                write!(
                    f,
                    "alias `{alias}` is not declared in `{path}` (root `{root}`)"
                )
            }
            DiagnosticKind::AliasDuplicate { root, path, alias } => {
                write!(
                    f,
                    "alias `{alias}` is declared more than once in `{path}` (root `{root}`)"
                )
            }
            DiagnosticKind::Malformed { kind, reason } => write!(f, "malformed {kind}: {reason}"),
            DiagnosticKind::Unverified(unverified) => match unverified {
                Unverified::RootAbsent { name, .. } => {
                    write!(
                        f,
                        "root `{name}` is not present; references into it were not checked"
                    )
                }
                Unverified::NoGrammar { path, .. } => {
                    write!(
                        f,
                        "no grammar for `{path}`; symbol references into it were not checked"
                    )
                }
                Unverified::ParseErrors { path, .. } => {
                    write!(
                        f,
                        "`{path}` has parse errors; missing symbols in it could not be confirmed"
                    )
                }
                Unverified::ParseTimeout { path, .. } => write!(f, "parsing `{path}` timed out"),
                Unverified::SymbolTableTruncated { path, .. } => {
                    write!(f, "`{path}` has too many declarations to index")
                }
                Unverified::TargetTooLarge {
                    path, bytes, limit, ..
                } => {
                    write!(
                        f,
                        "`{path}` is {bytes} bytes, over the {limit}-byte limit; not read"
                    )
                }
                Unverified::TargetNotUtf8 { path, .. } => {
                    write!(f, "`{path}` is not UTF-8; not read")
                }
                Unverified::TargetUnreadable { path, message, .. } => {
                    write!(f, "`{path}` could not be read: {message}")
                }
                Unverified::AnalyzeFailed { path, message, .. } => {
                    write!(f, "`{path}` could not be analyzed: {message}")
                }
            },
            DiagnosticKind::ExternalDuplicate { root, id } => {
                write!(
                    f,
                    "anchor id `{id}` is declared more than once in external root `{root}`"
                )
            }
            DiagnosticKind::FileSkipped { reason, .. } => write!(f, "file not checked: {reason}"),
            DiagnosticKind::WalkProblem { message, .. } => write!(f, "could not walk: {message}"),
        }
    }
}

/// A marker site with its line and column resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocatedSite {
    pub site: Site,
    pub line_col: LineCol,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileLocation {
    pub root: RootName,
    pub path: FilePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locations {
    Sites(Vec<LocatedSite>),
    Files(Vec<FileLocation>),
    Roots(Vec<RootName>),
}

impl Locations {
    pub fn len(&self) -> usize {
        match self {
            Locations::Sites(sites) => sites.len(),
            Locations::Files(files) => files.len(),
            Locations::Roots(roots) => roots.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub severity: Severity,
    pub locations: Locations,
    pub suggestion: Option<String>,
    /// Context that does not change the cause, such as how many alias uses a broken
    /// declaration carries.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub roots_scanned: usize,
    pub files_scanned: usize,
    pub anchors: usize,
    pub refs_checked: usize,
    pub refs_resolved: usize,
    /// `@[Alias]` sites bound to a declaration in their file; unbound ones are errors.
    pub alias_uses: usize,
    pub errors: usize,
    pub unverified: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    pub summary: Summary,
    pub policy: UnverifiedPolicy,
    /// Directory of every present root, so renderers can read source for snippets.
    pub root_dirs: BTreeMap<RootName, Utf8PathBuf>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.summary.errors > 0
    }
}

/// Accumulates findings and groups them by cause.
#[derive(Debug, Default)]
pub struct ReportBuilder {
    sites: HashMap<DiagnosticKind, Vec<LocatedSite>>,
    files: HashMap<DiagnosticKind, Vec<FileLocation>>,
    roots: HashMap<DiagnosticKind, Vec<RootName>>,
    suggestions: HashMap<DiagnosticKind, String>,
    notes: HashMap<DiagnosticKind, Vec<String>>,
    order: Vec<DiagnosticKind>,
}

impl ReportBuilder {
    pub fn site(&mut self, kind: DiagnosticKind, site: LocatedSite) {
        self.remember(&kind);
        self.sites.entry(kind).or_default().push(site);
    }

    pub fn file(&mut self, kind: DiagnosticKind, location: FileLocation) {
        self.remember(&kind);
        self.files.entry(kind).or_default().push(location);
    }

    pub fn root(&mut self, kind: DiagnosticKind, root: RootName) {
        self.remember(&kind);
        self.roots.entry(kind).or_default().push(root);
    }

    /// The first suggestion offered for a cause is kept; later ones for the same cause agree.
    pub fn suggestion(&mut self, kind: &DiagnosticKind, suggestion: Option<String>) {
        if let Some(suggestion) = suggestion {
            self.suggestions.entry(kind.clone()).or_insert(suggestion);
        }
    }

    pub fn note(&mut self, kind: &DiagnosticKind, note: String) {
        let notes = self.notes.entry(kind.clone()).or_default();
        if !notes.contains(&note) {
            notes.push(note);
        }
    }

    pub fn finish(
        mut self,
        policy: UnverifiedPolicy,
        mut summary: Summary,
        root_dirs: BTreeMap<RootName, Utf8PathBuf>,
    ) -> Report {
        let mut diagnostics = Vec::with_capacity(self.order.len());
        for kind in self.order {
            let locations = if let Some(mut sites) = self.sites.remove(&kind) {
                sites.sort();
                Locations::Sites(sites)
            } else if let Some(mut files) = self.files.remove(&kind) {
                files.sort();
                Locations::Files(files)
            } else {
                let mut roots = self.roots.remove(&kind).unwrap_or_default();
                roots.sort();
                Locations::Roots(roots)
            };
            let severity = match (kind.base_severity(), policy) {
                (Severity::Unverified, UnverifiedPolicy::Error) => Severity::Error,
                (severity, _) => severity,
            };
            let suggestion = self.suggestions.remove(&kind);
            let notes = self.notes.remove(&kind).unwrap_or_default();
            diagnostics.push(Diagnostic {
                kind,
                severity,
                locations,
                suggestion,
                notes,
            });
        }
        diagnostics.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| b.locations.len().cmp(&a.locations.len()))
                .then_with(|| a.kind.to_string().cmp(&b.kind.to_string()))
        });
        summary.errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        summary.unverified = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Unverified)
            .count();
        Report {
            diagnostics,
            summary,
            policy,
            root_dirs,
        }
    }

    fn remember(&mut self, kind: &DiagnosticKind) {
        let seen = self.sites.contains_key(kind)
            || self.files.contains_key(kind)
            || self.roots.contains_key(kind);
        if !seen {
            self.order.push(kind.clone());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
