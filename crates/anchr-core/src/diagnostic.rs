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
    /// @ref[crates/anchr-core/src/resolve/mod.rs#Unverified] at report time.
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
mod tests {
    use camino::Utf8PathBuf;

    use super::*;
    use crate::span::ByteSpan;
    use crate::text::RegionKind;

    fn root() -> RootName {
        RootName::parse("r").unwrap()
    }

    fn located(path: &str, line: u32) -> LocatedSite {
        LocatedSite {
            site: Site {
                root: root(),
                path: FilePath::new(Utf8PathBuf::from(path)).unwrap(),
                span: ByteSpan::new(0, 5),
                region: RegionKind::Prose,
            },
            line_col: LineCol { line, col: 1 },
        }
    }

    fn missing(id: &str) -> DiagnosticKind {
        DiagnosticKind::Unresolved(Unresolved::AnchorMissing {
            root: root(),
            id: AnchorId::parse(id).unwrap(),
        })
    }

    #[test]
    fn sites_group_by_cause_and_sort_by_location() {
        let mut builder = ReportBuilder::default();
        builder.site(missing("auth/flow"), located("z.md", 3));
        builder.site(missing("auth/flow"), located("a.md", 9));
        builder.suggestion(&missing("auth/flow"), Some("auth/token-refresh".to_owned()));
        builder.site(missing("auth/flow"), located("a.md", 2));
        builder.site(missing("other"), located("b.md", 1));

        let report = builder.finish(
            UnverifiedPolicy::Report,
            Summary::default(),
            BTreeMap::new(),
        );
        assert_eq!(report.diagnostics.len(), 2);
        let first = &report.diagnostics[0];
        assert_eq!(first.kind, missing("auth/flow"));
        assert_eq!(first.suggestion.as_deref(), Some("auth/token-refresh"));
        let Locations::Sites(sites) = &first.locations else {
            panic!("expected sites");
        };
        let order: Vec<(String, u32)> = sites
            .iter()
            .map(|s| (s.site.path.to_string(), s.line_col.line))
            .collect();
        assert_eq!(
            order,
            vec![
                ("a.md".to_owned(), 2),
                ("a.md".to_owned(), 9),
                ("z.md".to_owned(), 3)
            ]
        );
        assert_eq!(report.summary.errors, 2);
        assert!(report.has_errors());
    }

    #[test]
    fn errors_sort_before_unverified_and_strict_promotes_them() {
        let unverified = DiagnosticKind::Unverified(Unverified::RootAbsent {
            name: RootName::parse("claude").unwrap(),
            declared_dir: Utf8PathBuf::from("/x"),
        });
        let mut builder = ReportBuilder::default();
        builder.site(unverified.clone(), located("a.md", 1));
        builder.site(unverified.clone(), located("a.md", 2));
        builder.site(missing("x"), located("b.md", 1));

        let lenient = builder.finish(
            UnverifiedPolicy::Report,
            Summary::default(),
            BTreeMap::new(),
        );
        assert_eq!(lenient.diagnostics[0].kind, missing("x"));
        assert_eq!(lenient.diagnostics[1].severity, Severity::Unverified);
        assert_eq!((lenient.summary.errors, lenient.summary.unverified), (1, 1));
        assert!(lenient.diagnostics[1].kind.hint().is_some());

        let mut builder = ReportBuilder::default();
        builder.site(unverified, located("a.md", 1));
        let strict = builder.finish(UnverifiedPolicy::Error, Summary::default(), BTreeMap::new());
        assert_eq!(strict.diagnostics[0].severity, Severity::Error);
        assert!(strict.has_errors());
    }

    #[test]
    fn file_and_root_level_findings_have_their_own_location_shapes() {
        let mut builder = ReportBuilder::default();
        let skipped = DiagnosticKind::FileSkipped {
            root: root(),
            reason: SkipReason::NotUtf8,
        };
        builder.file(
            skipped,
            FileLocation {
                root: root(),
                path: FilePath::new(Utf8PathBuf::from("bin.md")).unwrap(),
            },
        );
        let walk = DiagnosticKind::WalkProblem {
            root: root(),
            message: "permission denied".to_owned(),
        };
        builder.root(walk, root());
        let report = builder.finish(
            UnverifiedPolicy::Report,
            Summary::default(),
            BTreeMap::new(),
        );
        let skipped_diagnostic = report
            .diagnostics
            .iter()
            .find(|d| matches!(d.kind, DiagnosticKind::FileSkipped { .. }))
            .unwrap();
        assert!(matches!(skipped_diagnostic.locations, Locations::Files(ref f) if f.len() == 1));
        assert!(skipped_diagnostic.kind.hint().is_some());
        let walk_diagnostic = report
            .diagnostics
            .iter()
            .find(|d| matches!(d.kind, DiagnosticKind::WalkProblem { .. }))
            .unwrap();
        assert!(matches!(walk_diagnostic.locations, Locations::Roots(ref r) if r.len() == 1));
    }

    #[test]
    fn every_kind_has_a_human_title() {
        let title = missing("auth/flow").to_string();
        assert_eq!(title, "unknown anchor id `auth/flow` in root `r`");
        let malformed = DiagnosticKind::Malformed {
            kind: MarkerKind::Ref,
            reason: MalformedReason::Unclosed,
        };
        assert_eq!(
            malformed.to_string(),
            "malformed @ref: missing closing `]` on the same line"
        );
        let undeclared = DiagnosticKind::AliasUndeclared {
            root: RootName::parse("r").unwrap(),
            path: FilePath::new(camino::Utf8PathBuf::from("docs/x.md")).unwrap(),
            alias: Alias::parse("Analyser").unwrap(),
        };
        assert_eq!(
            undeclared.to_string(),
            "alias `Analyser` is not declared in `docs/x.md` (root `r`)"
        );
        assert_eq!(undeclared.base_severity(), Severity::Error);
        assert!(undeclared.hint().unwrap().contains("as Analyser]"));
    }
}
