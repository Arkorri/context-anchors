//! The demoted heuristic scanner. Opt-in markers give soundness on what is annotated and
//! nothing on what is not; `coverage` reports reference-shaped strings that carry no marker,
//! and `annotate` proposes markers only where the target already resolves. It never errors
//! and never writes on its own.

mod linguist;
mod token;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::check::Workspace;
use crate::edit::TextEdit;
use crate::index::Site;
use crate::marker::{Alias, MarkerPayload, NoRefEntry, NoRefItem, parse_target};
use crate::noref::NoRefSet;
use crate::resolve::{Resolution, Resolver};
use crate::root::{FilePath, RootName};
use crate::span::ByteSpan;
use crate::text::{Container, FileAnalyzer, RegionKind};
use token::{KnownExtensions, PathShape, Shape, tokens};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// The target resolves; replacing the token with this marker would add a checked
    /// reference without introducing an error.
    Proposal { replacement: String },
    /// Reference-shaped but does not resolve: quite possibly a stale reference in prose.
    Unresolvable { reason: String },
    /// An identifier declared in more than one scanned file; a human must pick.
    Ambiguous { declared_in: Vec<FilePath> },
    /// Declared with `as` in this file and never written as `@[alias]`. Advisory only, never
    /// an edit: the fix is to use it or drop the clause.
    UnusedAlias { alias: Alias },
    /// Listed in this file's `@noref[...]` and matched nothing. Advisory only: the fix is to
    /// drop the entry.
    UnusedIgnore { entry: NoRefEntry },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The bytes a proposal would replace: the token, or the whole code span when the token
    /// is all the code span holds (a marker inside backticks would not be checked).
    pub site: Site,
    pub text: String,
    pub kind: CandidateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoverageSummary {
    /// Direct references plus bound alias uses: every checked mention.
    pub annotated_refs: usize,
    pub proposals: usize,
    pub unresolvable: usize,
    pub ambiguous: usize,
    /// Not reference-shaped strings, so not part of `total`.
    pub unused_aliases: usize,
    /// Tokens an ignore list suppressed. The author has said they are not references, so they
    /// are not part of `total` either.
    pub ignored: usize,
    /// `@noref` entries plus `[coverage] ignore` entries that matched nothing.
    pub unused_ignores: usize,
}

impl CoverageSummary {
    /// Reference-shaped strings, annotated or not.
    pub fn total(&self) -> usize {
        self.annotated_refs + self.proposals + self.unresolvable + self.ambiguous
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    pub candidates: Vec<Candidate>,
    /// `[coverage] ignore` entries that matched nothing. Not candidates: they have no site in
    /// an indexed file.
    pub unused_config_ignores: Vec<NoRefEntry>,
    pub summary: CoverageSummary,
}

impl CoverageReport {
    /// The edits `annotate` would make, per file, sorted by span.
    pub fn proposals(&self) -> BTreeMap<FilePath, Vec<TextEdit>> {
        let mut edits: BTreeMap<FilePath, Vec<TextEdit>> = BTreeMap::new();
        for candidate in &self.candidates {
            if let CandidateKind::Proposal { replacement } = &candidate.kind {
                edits
                    .entry(candidate.site.path.clone())
                    .or_default()
                    .push(TextEdit {
                        span: candidate.site.span,
                        expected: candidate.text.clone(),
                        replacement: replacement.clone(),
                    });
            }
        }
        for file_edits in edits.values_mut() {
            file_edits.sort_by_key(|edit| edit.span.start);
        }
        edits
    }
}

/// Scans the current root. `only_files` and `[coverage] exclude` narrow which files are scanned
/// for candidates and counted; the symbol index used to place identifiers always covers the
/// whole root. A file that cannot be read or analyzed is skipped: coverage informs, it never
/// fails.
pub fn coverage(workspace: &Workspace, only_files: &[FilePath]) -> CoverageReport {
    let (root, index) = workspace.current();
    let mut analyzer = FileAnalyzer::new(&workspace.registry, root.config.scan.parse_budget);
    let symbols = symbol_index(workspace, &mut analyzer);
    let mut resolver = Resolver::new(&workspace.roots, &workspace.registry);
    let no_extensions = BTreeSet::new();
    let known = KnownExtensions::new(
        workspace
            .findings
            .get(&root.name)
            .map_or(&no_extensions, |findings| &findings.extensions),
    );
    let excluded = &root.config.coverage.exclude;
    let in_scope = |path: &FilePath| {
        (only_files.is_empty() || only_files.contains(path)) && !excluded.is_match(path.as_path())
    };
    let mut global = NoRefSet::new(root.config.coverage.ignore.iter().cloned());

    let mut paths: Vec<&FilePath> = index.file_paths().filter(|path| in_scope(path)).collect();
    paths.sort();

    let mut candidates = Vec::new();
    let mut ignored = 0;
    for path in paths {
        let Some(record) = index.file_record(path) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(root.dir.join(path.as_path())) else {
            continue;
        };
        let Some(container) =
            Container::for_path(path.as_path(), &root.config.containers, &workspace.registry)
        else {
            continue;
        };
        let Ok(regions) = analyzer.coverage_regions(container, &source) else {
            continue;
        };
        let mut occupied: Vec<ByteSpan> = record.markers.iter().map(|m| m.span).collect();
        occupied.extend(record.malformed.iter().map(|m| m.span));
        let aliases: Vec<&Alias> = record.aliases.aliases().collect();
        let noref_items: Vec<&NoRefItem> = record
            .markers
            .iter()
            .filter_map(|marker| match &marker.payload {
                MarkerPayload::NoRef { items } => Some(items),
                MarkerPayload::Anchor { .. }
                | MarkerPayload::Ref { .. }
                | MarkerPayload::Use { .. } => None,
            })
            .flatten()
            .collect();
        let mut local = NoRefSet::new(noref_items.iter().map(|item| item.entry.clone()));

        for region in regions.iter() {
            let Some(text) = source.get(region.span.start..region.span.end) else {
                continue;
            };
            for token in tokens(text, region.kind, &aliases, known) {
                let span = token.span.shifted_by(region.span.start);
                if occupied.iter().any(|taken| taken.intersects(span)) {
                    continue;
                }
                if local.claim(&token.text) || global.claim(&token.text) {
                    ignored += 1;
                    continue;
                }
                let kind = match &token.shape {
                    Shape::Path(shape) => {
                        classify_path(&mut resolver, &root.name, &token.text, *shape)
                    }
                    Shape::Identifier => classify_identifier(&symbols, &token.text),
                    Shape::Alias(alias) => Some(CandidateKind::Proposal {
                        replacement: format!("@[{alias}]"),
                    }),
                };
                let Some(kind) = kind else {
                    continue;
                };
                candidates.push(Candidate {
                    site: Site {
                        root: root.name.clone(),
                        path: path.clone(),
                        span,
                        region: region.kind,
                    },
                    text: source
                        .get(span.start..span.end)
                        .unwrap_or(&token.text)
                        .to_owned(),
                    kind,
                });
            }
        }
        for (position, entry) in local.unused() {
            let item = noref_items[position];
            let region = record
                .markers
                .iter()
                .find(|marker| marker.span.contains(item.span.start))
                .map_or(RegionKind::Prose, |marker| marker.region);
            candidates.push(Candidate {
                site: Site {
                    root: root.name.clone(),
                    path: path.clone(),
                    span: item.span,
                    region,
                },
                text: entry.to_string(),
                kind: CandidateKind::UnusedIgnore {
                    entry: entry.clone(),
                },
            });
        }
    }

    let mut unused: Vec<(&FilePath, &Alias, ByteSpan)> = index
        .unused_aliases()
        .filter(|(path, _, _)| in_scope(path))
        .map(|(path, alias, binding)| (path, alias, binding.alias_span))
        .collect();
    unused.sort();
    for (path, alias, alias_span) in unused {
        let region = index
            .file_record(path)
            .and_then(|record| {
                record
                    .markers
                    .iter()
                    .find(|marker| marker.span.contains(alias_span.start))
            })
            .map_or(RegionKind::Prose, |marker| marker.region);
        candidates.push(Candidate {
            site: Site {
                root: root.name.clone(),
                path: path.clone(),
                span: alias_span,
                region,
            },
            text: alias.to_string(),
            kind: CandidateKind::UnusedAlias {
                alias: alias.clone(),
            },
        });
    }

    let direct_refs = index
        .refs()
        .filter(|ref_site| in_scope(&ref_site.site.path))
        .count();
    let bound_uses = index
        .alias_uses()
        .filter(|use_site| use_site.binding.is_some() && in_scope(&use_site.site.path))
        .count();
    // A narrowed run cannot see the files where a root-wide entry matches, so only a whole-root
    // run can call one unused.
    let unused_config_ignores: Vec<NoRefEntry> = if only_files.is_empty() {
        global.unused().map(|(_, entry)| entry.clone()).collect()
    } else {
        Vec::new()
    };
    let mut summary = CoverageSummary {
        annotated_refs: direct_refs + bound_uses,
        ignored,
        unused_ignores: unused_config_ignores.len(),
        ..CoverageSummary::default()
    };
    for candidate in &candidates {
        match candidate.kind {
            CandidateKind::Proposal { .. } => summary.proposals += 1,
            CandidateKind::Unresolvable { .. } => summary.unresolvable += 1,
            CandidateKind::Ambiguous { .. } => summary.ambiguous += 1,
            CandidateKind::UnusedAlias { .. } => summary.unused_aliases += 1,
            CandidateKind::UnusedIgnore { .. } => summary.unused_ignores += 1,
        }
    }
    CoverageReport {
        candidates,
        unused_config_ignores,
        summary,
    }
}

/// A glob tail names the directory it globs, so `src/**` is checked and proposed as `src/`.
fn classify_path(
    resolver: &mut Resolver<'_>,
    current: &RootName,
    token: &str,
    shape: PathShape,
) -> Option<CandidateKind> {
    let target_text = match shape {
        PathShape::Glob => token.trim_end_matches('*'),
        PathShape::File | PathShape::Directory => token,
    };
    let target = parse_target(target_text).ok()?.target;
    Some(match resolver.resolve(current, &target) {
        Resolution::Resolved => CandidateKind::Proposal {
            replacement: format!("@ref[{target_text}]"),
        },
        Resolution::Unresolved(unresolved) => CandidateKind::Unresolvable {
            reason: crate::diagnostic::DiagnosticKind::Unresolved(unresolved).to_string(),
        },
        Resolution::Unverified(unverified) => CandidateKind::Unresolvable {
            reason: crate::diagnostic::DiagnosticKind::Unverified(unverified).to_string(),
        },
    })
}

fn classify_identifier(
    symbols: &HashMap<String, Vec<FilePath>>,
    name: &str,
) -> Option<CandidateKind> {
    let declared_in = symbols.get(name)?;
    match declared_in.as_slice() {
        [] => None,
        [only] => Some(CandidateKind::Proposal {
            replacement: format!("@ref[{only}#{name}]"),
        }),
        many => Some(CandidateKind::Ambiguous {
            declared_in: many.to_vec(),
        }),
    }
}

/// Declaration name → files declaring it, over every source file in the current root.
fn symbol_index(
    workspace: &Workspace,
    analyzer: &mut FileAnalyzer<'_>,
) -> HashMap<String, Vec<FilePath>> {
    let (root, index) = workspace.current();
    let mut symbols: HashMap<String, Vec<FilePath>> = HashMap::new();
    let mut paths: Vec<&FilePath> = index.file_paths().collect();
    paths.sort();
    for path in paths {
        let Some(Container::Source(spec)) =
            Container::for_path(path.as_path(), &root.config.containers, &workspace.registry)
        else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(root.dir.join(path.as_path())) else {
            continue;
        };
        let Ok(table) = analyzer.symbols(spec, &source) else {
            continue;
        };
        for name in table.names() {
            symbols
                .entry(name.to_owned())
                .or_default()
                .push(path.clone());
        }
    }
    symbols
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;

    use super::*;
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

        fn coverage(&self) -> CoverageReport {
            let workspace = Workspace::load(config::discover(&self.root_dir).unwrap()).unwrap();
            coverage(&workspace, &[])
        }
    }

    fn describe(report: &CoverageReport) -> Vec<(String, String, String)> {
        report
            .candidates
            .iter()
            .map(|c| {
                let kind = match &c.kind {
                    CandidateKind::Proposal { replacement } => format!("propose {replacement}"),
                    CandidateKind::Unresolvable { .. } => "unresolvable".to_owned(),
                    CandidateKind::Ambiguous { declared_in } => {
                        format!("ambiguous x{}", declared_in.len())
                    }
                    CandidateKind::UnusedAlias { .. } => "unused alias".to_owned(),
                    CandidateKind::UnusedIgnore { .. } => "unused ignore".to_owned(),
                };
                (c.site.path.to_string(), c.text.clone(), kind)
            })
            .collect()
    }

    fn row(path: &str, text: &str, kind: &str) -> (String, String, String) {
        (path.to_owned(), text.to_owned(), kind.to_owned())
    }

    #[test]
    fn paths_in_prose_and_code_spans_are_proposed_when_they_resolve() {
        let fixture = Fixture::new(&[
            (
                "README.md",
                "See `docs/guide.md`, docs/guide.md, and docs/missing.md. Config in anchr.toml.\nAlready @ref[docs/guide.md]. Not https://example.com/docs/guide.md nor v1/2.\n",
            ),
            ("docs/guide.md", "# Guide\n"),
            ("anchr.toml", ""),
        ]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row(
                    "README.md",
                    "`docs/guide.md`",
                    "propose @ref[docs/guide.md]"
                ),
                row("README.md", "docs/guide.md", "propose @ref[docs/guide.md]"),
                row("README.md", "docs/missing.md", "unresolvable"),
                row("README.md", "anchr.toml", "propose @ref[anchr.toml]"),
            ]
        );
        assert_eq!(report.summary.annotated_refs, 1);
        assert_eq!(report.summary.proposals, 3);
        assert_eq!(report.summary.unresolvable, 1);
        assert_eq!(report.summary.total(), 5);
    }

    #[test]
    fn directory_and_glob_tokens_propose_the_directory_form() {
        let fixture = Fixture::new(&[
            (
                "README.md",
                "See `docs/research/`, src/* and src/**. Also the missing/ directory.\n",
            ),
            ("docs/research/survey.md", ""),
            ("src/lib.rs", ""),
        ]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row(
                    "README.md",
                    "`docs/research/`",
                    "propose @ref[docs/research/]"
                ),
                row("README.md", "src/*", "propose @ref[src/]"),
                row("README.md", "src/**", "propose @ref[src/]"),
                row("README.md", "missing/", "unresolvable"),
            ]
        );
        let edits = report.proposals();
        let readme = &edits[&FilePath::new(Utf8PathBuf::from("README.md")).unwrap()];
        let described: Vec<(&str, &str)> = readme
            .iter()
            .map(|edit| (edit.expected.as_str(), edit.replacement.as_str()))
            .collect();
        assert_eq!(
            described,
            vec![
                ("`docs/research/`", "@ref[docs/research/]"),
                ("src/*", "@ref[src/]"),
                ("src/**", "@ref[src/]"),
            ]
        );
    }

    #[test]
    fn prose_slash_pairs_and_version_numbers_are_not_candidates() {
        let fixture = Fixture::new(&[(
            "README.md",
            "Reports line/col, licensed Apache-2.0/MIT, e.g. since 1.x, i.e. a.k.a. v1.1;\nsee src/directory and the website, or `struct/class` and `e.g`.\n",
        )]);
        let report = fixture.coverage();
        assert!(report.candidates.is_empty(), "{:?}", describe(&report));
        assert_eq!(report.summary.total(), 0);
    }

    #[test]
    fn repo_extensions_extend_the_linguist_table() {
        let with_file = Fixture::new(&[
            ("README.md", "Read other.zzq first.\n"),
            ("data/notes.zzq", ""),
        ]);
        assert_eq!(
            describe(&with_file.coverage()),
            vec![row("README.md", "other.zzq", "unresolvable")]
        );

        let without_file = Fixture::new(&[("README.md", "Read other.zzq first.\n")]);
        assert!(without_file.coverage().candidates.is_empty());
    }

    #[test]
    fn linguist_extensions_are_known_without_a_repo_file() {
        let fixture = Fixture::new(&[("README.md", "The notes moved from old.txt.\n")]);
        assert_eq!(
            describe(&fixture.coverage()),
            vec![row("README.md", "old.txt", "unresolvable")]
        );
    }

    #[test]
    fn identifiers_in_code_spans_resolve_through_the_symbol_index() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "Call `validate_token`, then `shared_name`; `not_declared` and `x` are ignored.\n",
            ),
            (
                "src/auth.rs",
                "pub fn validate_token() {}\npub fn shared_name() {}\n",
            ),
            (
                "src/other.rs",
                "pub fn shared_name() {}\n// see `validate_token` and src/auth.rs\n",
            ),
        ]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row(
                    "docs/a.md",
                    "`validate_token`",
                    "propose @ref[src/auth.rs#validate_token]"
                ),
                row("docs/a.md", "`shared_name`", "ambiguous x2"),
                row(
                    "src/other.rs",
                    "`validate_token`",
                    "propose @ref[src/auth.rs#validate_token]"
                ),
                row("src/other.rs", "src/auth.rs", "propose @ref[src/auth.rs]"),
            ]
        );
        assert_eq!(report.summary.ambiguous, 1);
    }

    #[test]
    fn alias_words_are_proposed_per_file_and_unused_aliases_are_advisories() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "@ref[docs/guide.md as Guide] @ref[docs/guide.md as Spare]\nRead the Guide, or `Guide`; not [Guide](docs/guide.md), Guidebook, or guide. @[Guide] is done.\n",
            ),
            ("docs/b.md", "Guide is just a word here; so is `Guide`.\n"),
            ("docs/guide.md", "# Guide\n"),
            (
                "src/x.rs",
                "// The Guide struct: `Guide` and Guide\npub struct Guide;\n",
            ),
        ]);
        let report = fixture.coverage();
        let described = describe(&report);
        assert_eq!(
            described
                .iter()
                .filter(|(path, _, kind)| path == "docs/a.md" && kind == "propose @[Guide]")
                .map(|(_, text, _)| text.as_str())
                .collect::<Vec<_>>(),
            vec!["Guide", "`Guide`"]
        );
        assert_eq!(
            described
                .iter()
                .filter(|(path, _, _)| path == "docs/a.md")
                .filter(|(_, _, kind)| kind == "unused alias")
                .map(|(_, text, _)| text.as_str())
                .collect::<Vec<_>>(),
            vec!["Spare"]
        );
        assert!(
            described
                .iter()
                .filter(|(path, _, _)| path != "docs/a.md")
                .all(|(_, _, kind)| !kind.contains("@[")),
            "{described:?}"
        );
        assert_eq!(report.summary.annotated_refs, 3);
        assert_eq!(report.summary.unused_aliases, 1);
        assert_eq!(
            report.summary.total(),
            report.summary.annotated_refs
                + report.summary.proposals
                + report.summary.unresolvable
                + report.summary.ambiguous
        );
        let edits = report.proposals();
        let a = &edits[&FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap()];
        assert!(a.iter().all(|edit| edit.replacement == "@[Guide]"));
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn links_and_fences_are_never_candidates_and_proposals_become_edits() {
        let fixture = Fixture::new(&[
            (
                "README.md",
                "[guide](docs/guide.md) and\n\n```\ndocs/guide.md\n```\n\nplus `docs/guide.md`.\n",
            ),
            ("docs/guide.md", ""),
        ]);
        let report = fixture.coverage();
        assert_eq!(report.candidates.len(), 1);
        let edits = report.proposals();
        let readme = &edits[&FilePath::new(Utf8PathBuf::from("README.md")).unwrap()];
        assert_eq!(readme.len(), 1);
        assert_eq!(readme[0].expected, "`docs/guide.md`");
        assert_eq!(readme[0].replacement, "@ref[docs/guide.md]");
    }

    #[test]
    fn noref_lists_suppress_every_token_shape_in_their_file_only() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "@noref[docs/guide.md, src/, Guide]\n@ref[docs/guide.md as Guide]\nSee `docs/guide.md`, src/x.rs, `src/x.rs#run`, src/*, src/, and Guide (@[Guide]).\n",
            ),
            ("docs/b.md", "See `docs/guide.md` and src/x.rs.\n"),
            ("docs/guide.md", "# Guide\n"),
            ("src/x.rs", "pub fn run() {}\n"),
        ]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row(
                    "docs/b.md",
                    "`docs/guide.md`",
                    "propose @ref[docs/guide.md]"
                ),
                row("docs/b.md", "src/x.rs", "propose @ref[src/x.rs]"),
            ]
        );
        assert_eq!(report.summary.ignored, 6);
        assert_eq!(report.summary.unused_ignores, 0);
        assert_eq!(report.summary.annotated_refs, 2);
        assert_eq!(report.summary.total(), 4);
        assert!(report.unused_config_ignores.is_empty());
    }

    #[test]
    fn unused_and_duplicate_noref_entries_are_advisories_at_their_span() {
        let source = "@noref[foo.ts, bar.md, foo.ts]\nMentions foo.ts only.\n";
        let fixture = Fixture::new(&[("docs/a.md", source)]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row("docs/a.md", "bar.md", "unused ignore"),
                row("docs/a.md", "foo.ts", "unused ignore"),
            ]
        );
        let spans: Vec<&str> = report
            .candidates
            .iter()
            .map(|c| &source[c.site.span.start..c.site.span.end])
            .collect();
        assert_eq!(spans, vec!["bar.md", "foo.ts"]);
        assert_eq!(report.candidates[1].site.span.start, 23);
        assert_eq!(report.summary.ignored, 1);
        assert_eq!(report.summary.unused_ignores, 2);
        assert!(report.proposals().is_empty());
    }

    #[test]
    fn config_ignore_and_exclude_apply_across_files_and_report_unused_entries() {
        let fixture = Fixture::new(&[
            (
                "anchr.toml",
                "[coverage]\nexclude = [\"archive/**\"]\nignore = [\"CLAUDE.md\", \"never.md\"]\n",
            ),
            (
                "docs/a.md",
                "See `CLAUDE.md` and `docs/guide.md`. @ref[docs/guide.md]\n",
            ),
            (
                "archive/old.md",
                "See `docs/guide.md` and CLAUDE.md. @ref[docs/guide.md]\n",
            ),
            ("docs/guide.md", "# Guide\n"),
        ]);
        let workspace = Workspace::load(config::discover(&fixture.root_dir).unwrap()).unwrap();
        let report = coverage(&workspace, &[]);
        assert_eq!(
            describe(&report),
            vec![row(
                "docs/a.md",
                "`docs/guide.md`",
                "propose @ref[docs/guide.md]"
            )]
        );
        assert_eq!(
            report
                .unused_config_ignores
                .iter()
                .map(NoRefEntry::as_str)
                .collect::<Vec<_>>(),
            vec!["never.md"]
        );
        assert_eq!(report.summary.ignored, 1);
        assert_eq!(report.summary.unused_ignores, 1);
        assert_eq!(report.summary.annotated_refs, 1);
        assert_eq!(report.summary.total(), 2);

        let (_, index) = workspace.current();
        assert_eq!(index.refs().count(), 2, "excluded files are still indexed");
        let archive = FilePath::new(Utf8PathBuf::from("archive/old.md")).unwrap();
        let narrowed = coverage(&workspace, &[archive]);
        assert!(narrowed.candidates.is_empty());
        assert!(
            narrowed.unused_config_ignores.is_empty(),
            "a narrowed run cannot judge root-wide entries"
        );
        assert_eq!(narrowed.summary.unused_ignores, 0);
    }

    #[test]
    fn a_local_entry_shadows_a_global_one() {
        let fixture = Fixture::new(&[
            ("anchr.toml", "[coverage]\nignore = [\"foo.ts\"]\n"),
            ("docs/a.md", "@noref[foo.ts]\nSee foo.ts.\n"),
        ]);
        let report = fixture.coverage();
        assert!(report.candidates.is_empty());
        assert_eq!(report.summary.ignored, 1);
        assert_eq!(report.unused_config_ignores.len(), 1);
    }
}
