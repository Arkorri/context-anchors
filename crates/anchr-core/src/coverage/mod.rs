//! The demoted heuristic scanner. Opt-in markers give soundness on what is annotated and
//! nothing on what is not; `coverage` reports reference-shaped strings that carry no marker,
//! and `annotate` proposes markers only where the target already resolves. It never errors
//! and never writes on its own.
//!
//! Only paths and declared aliases are candidates. A resolving path has one referent, so a
//! proposal for it is the reference the author meant; a bare code symbol does not, so it is
//! never a candidate. Symbols enter coverage through an alias declaration, after which every
//! use in that file is proposed.

mod linguist;
mod token;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use camino::Utf8Path;

use crate::check::Workspace;
use crate::diagnostic::DiagnosticKind;
use crate::edit::TextEdit;
use crate::index::Site;
use crate::marker::{
    Alias, MarkerPayload, NoRefEntry, NoRefItem, RefTarget, RelPath, parse_target,
};
use crate::noref::NoRefSet;
use crate::resolve::{Resolution, Resolver, Unresolved};
use crate::root::{FilePath, Root};
use crate::span::ByteSpan;
use crate::text::{Container, FileAnalyzer, RegionKind};
use crate::tree::FileTree;
use token::{KnownExtensions, PathShape, Shape, tokens};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// The target resolves; replacing the token with this marker would add a checked
    /// reference without introducing an error.
    Proposal { replacement: String },
    /// Reference-shaped but does not resolve: quite possibly a stale reference in prose.
    Unresolvable { reason: String },
    /// Declared with `as` in this file and never written as `@[alias]`. Advisory only, never
    /// an edit: the fix is to use it or drop the clause.
    UnusedAlias { alias: Alias },
    /// Listed in this file's `@noref[...]` and matched nothing. Advisory only: the fix is to
    /// drop the entry.
    UnusedIgnore { entry: NoRefEntry },
}

impl CandidateKind {
    /// Report order: what can be acted on first, advisories last.
    fn rank(&self) -> u8 {
        match self {
            CandidateKind::Proposal { .. } => 0,
            CandidateKind::Unresolvable { .. } => 1,
            CandidateKind::UnusedAlias { .. } => 2,
            CandidateKind::UnusedIgnore { .. } => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSite {
    pub site: Site,
    /// The bytes a proposal would replace: the token, or the whole code span when the token
    /// is all the code span holds (a marker inside backticks would not be checked).
    pub text: String,
}

/// Every site where one token received one verdict. Grouping mirrors `check`, which groups
/// diagnostics by cause: a header comment repeated in a thousand generated files is one entry
/// with a thousand sites, not a thousand lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateGroup {
    pub kind: CandidateKind,
    /// The token as the author wrote it, without the backticks a code span adds.
    pub token: String,
    /// Sorted by root, path, and span.
    pub sites: Vec<CandidateSite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoverageSummary {
    /// Direct references plus bound alias uses: every checked mention.
    pub annotated_refs: usize,
    pub proposals: usize,
    pub unresolvable: usize,
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
        self.annotated_refs + self.proposals + self.unresolvable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    /// Proposals first, then unresolvable strings, then the advisories; within a kind the
    /// groups with the most sites come first, ties broken by token.
    pub candidates: Vec<CandidateGroup>,
    /// `[coverage] ignore` entries that matched nothing. Not candidates: they have no site in
    /// an indexed file.
    pub unused_config_ignores: Vec<NoRefEntry>,
    pub summary: CoverageSummary,
}

impl CoverageReport {
    /// The edits `annotate` would make, per file, sorted by span.
    pub fn proposals(&self) -> BTreeMap<FilePath, Vec<TextEdit>> {
        let mut edits: BTreeMap<FilePath, Vec<TextEdit>> = BTreeMap::new();
        for group in &self.candidates {
            let CandidateKind::Proposal { replacement } = &group.kind else {
                continue;
            };
            for candidate in &group.sites {
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

/// One classified token at one site, before grouping.
struct Finding {
    kind: CandidateKind,
    token: String,
    site: CandidateSite,
}

fn group(findings: Vec<Finding>) -> Vec<CandidateGroup> {
    let mut by_key: HashMap<(CandidateKind, String), Vec<CandidateSite>> = HashMap::new();
    for finding in findings {
        by_key
            .entry((finding.kind, finding.token))
            .or_default()
            .push(finding.site);
    }
    let mut groups: Vec<CandidateGroup> = by_key
        .into_iter()
        .map(|((kind, token), mut sites)| {
            sites.sort_by(|a, b| a.site.cmp(&b.site));
            CandidateGroup { kind, token, sites }
        })
        .collect();
    groups.sort_by(|a, b| {
        a.kind
            .rank()
            .cmp(&b.kind.rank())
            .then_with(|| b.sites.len().cmp(&a.sites.len()))
            .then_with(|| a.token.cmp(&b.token))
            .then_with(|| {
                let first = |group: &CandidateGroup| group.sites.first().map(|c| c.site.clone());
                first(a).cmp(&first(b))
            })
    });
    groups
}

/// Scans the current root. `only_files` and `[coverage] exclude` narrow which files are scanned
/// for candidates and counted. A file that cannot be read or analyzed is skipped: coverage
/// informs, it never fails.
pub fn coverage(workspace: &Workspace, only_files: &[FilePath]) -> CoverageReport {
    let (root, index) = workspace.current();
    let mut analyzer = FileAnalyzer::new(&workspace.registry, root.config.scan.parse_budget);
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

    let mut findings = Vec::new();
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
                        classify_path(&mut resolver, root, index.tree(), path, &token.text, *shape)
                    }
                    Shape::Alias(alias) => Some(CandidateKind::Proposal {
                        replacement: format!("@[{alias}]"),
                    }),
                };
                let Some(kind) = kind else {
                    continue;
                };
                findings.push(Finding {
                    kind,
                    token: token.text.clone(),
                    site: CandidateSite {
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
                    },
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
            findings.push(Finding {
                kind: CandidateKind::UnusedIgnore {
                    entry: entry.clone(),
                },
                token: entry.to_string(),
                site: CandidateSite {
                    site: Site {
                        root: root.name.clone(),
                        path: path.clone(),
                        span: item.span,
                        region,
                    },
                    text: entry.to_string(),
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
        findings.push(Finding {
            kind: CandidateKind::UnusedAlias {
                alias: alias.clone(),
            },
            token: alias.to_string(),
            site: CandidateSite {
                site: Site {
                    root: root.name.clone(),
                    path: path.clone(),
                    span: alias_span,
                    region,
                },
                text: alias.to_string(),
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
    for finding in &findings {
        match finding.kind {
            CandidateKind::Proposal { .. } => summary.proposals += 1,
            CandidateKind::Unresolvable { .. } => summary.unresolvable += 1,
            CandidateKind::UnusedAlias { .. } => summary.unused_aliases += 1,
            CandidateKind::UnusedIgnore { .. } => summary.unused_ignores += 1,
        }
    }
    CoverageReport {
        candidates: group(findings),
        unused_config_ignores,
        summary,
    }
}

/// A glob tail names the directory it globs, so `src/**` is checked and proposed as `src/`. A
/// bare token is root-relative first; one that misses may have been written the way Markdown
/// links are, relative to the file, and falls through to `relative_fallback`.
fn classify_path(
    resolver: &mut Resolver<'_>,
    root: &Root,
    tree: &FileTree,
    file: &FilePath,
    token: &str,
    shape: PathShape,
) -> Option<CandidateKind> {
    let target_text = match shape {
        PathShape::Glob => token.trim_end_matches('*'),
        PathShape::File | PathShape::Directory => token,
    };
    // `./` and `../` alone name the writing file's own directory or its parent, which always
    // exist: prose about the syntax, never a reference.
    if target_text.chars().all(|ch| ch == '.' || ch == '/') {
        return None;
    }
    let target = parse_target(target_text, Some(file)).ok()?.target;
    Some(match resolver.resolve(&root.name, &target) {
        Resolution::Resolved => CandidateKind::Proposal {
            replacement: format!("@ref[{target_text}]"),
        },
        Resolution::Unresolved(missing @ Unresolved::PathMissing { .. })
            if target.root().is_none() =>
        {
            relative_fallback(resolver, root, tree, file, target_text, &target, missing)
        }
        Resolution::Unresolved(unresolved) => unresolvable(unresolved),
        Resolution::Unverified(unverified) => CandidateKind::Unresolvable {
            reason: DiagnosticKind::Unverified(unverified).to_string(),
        },
    })
}

fn unresolvable(unresolved: Unresolved) -> CandidateKind {
    CandidateKind::Unresolvable {
        reason: DiagnosticKind::Unresolved(unresolved).to_string(),
    }
}

/// Same directory proposes the `./` form with the author's spelling kept; exactly one ancestor
/// proposes the root-relative path; otherwise the token stays unresolvable, naming the one file
/// of that name if there is exactly one. Resolution goes through the resolver so directory
/// expectations and symbol lookups keep their meaning.
fn relative_fallback(
    resolver: &mut Resolver<'_>,
    root: &Root,
    tree: &FileTree,
    file: &FilePath,
    target_text: &str,
    target: &RefTarget,
    missing: Unresolved,
) -> CandidateKind {
    let Some(path) = target.path() else {
        return unresolvable(missing);
    };
    let tail = target_text.strip_prefix(path.as_str()).unwrap_or("");
    let mut resolves_under = |base: &Utf8Path| -> Option<RelPath> {
        let joined = if base.as_str().is_empty() {
            path.as_str().to_owned()
        } else {
            format!("{base}/{path}")
        };
        let candidate = RelPath::parse(&joined).ok()?;
        let resolution = resolver.resolve(&root.name, &target.with_path(candidate.clone()));
        (resolution == Resolution::Resolved).then_some(candidate)
    };

    let dir = file.directory();
    if !dir.as_str().is_empty() && resolves_under(dir).is_some() {
        return CandidateKind::Proposal {
            replacement: format!("@ref[./{target_text}]"),
        };
    }
    let mut hits = dir
        .ancestors()
        .skip(1)
        .filter(|ancestor| !ancestor.as_str().is_empty())
        .filter_map(resolves_under);
    if let (Some(candidate), None) = (hits.next(), hits.next()) {
        return CandidateKind::Proposal {
            replacement: format!("@ref[{candidate}{tail}]"),
        };
    }

    let mut reason = DiagnosticKind::Unresolved(missing).to_string();
    if let [found] = tree.by_basename(path.file_name())[..] {
        reason.push_str(&format!(
            "; one file named `{}` exists at `{found}`",
            path.file_name()
        ));
    }
    CandidateKind::Unresolvable { reason }
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

    /// Every site in file order, so tests read like the fixture text.
    fn sites(report: &CoverageReport) -> Vec<(&CandidateKind, &CandidateSite)> {
        let mut sites: Vec<(&CandidateKind, &CandidateSite)> = report
            .candidates
            .iter()
            .flat_map(|group| group.sites.iter().map(move |site| (&group.kind, site)))
            .collect();
        sites.sort_by(|a, b| a.1.site.cmp(&b.1.site));
        sites
    }

    fn describe(report: &CoverageReport) -> Vec<(String, String, String)> {
        sites(report)
            .into_iter()
            .map(|(kind, c)| {
                let kind = match kind {
                    CandidateKind::Proposal { replacement } => format!("propose {replacement}"),
                    CandidateKind::Unresolvable { .. } => "unresolvable".to_owned(),
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
    fn backticked_identifiers_are_never_candidates() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "Call `validate_token`, then `shared_name`; see `src/auth.rs#validate_token`.\n",
            ),
            (
                "src/auth.rs",
                "pub fn validate_token() {}\npub fn shared_name() {}\n",
            ),
            (
                "src/other.rs",
                "pub fn shared_name() {}\n// see `validate_token`, `shared_name`, and src/auth.rs\n",
            ),
        ]);
        let report = fixture.coverage();
        assert_eq!(
            describe(&report),
            vec![
                row(
                    "docs/a.md",
                    "`src/auth.rs#validate_token`",
                    "propose @ref[src/auth.rs#validate_token]"
                ),
                row("src/other.rs", "src/auth.rs", "propose @ref[src/auth.rs]"),
            ]
        );
        assert_eq!(report.summary.total(), 2);
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
            report.summary.annotated_refs + report.summary.proposals + report.summary.unresolvable
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
        // `src/**` in a.md claims the subtree; the bare `src/` in b.md claims only that token.
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "@noref[docs/guide.md, src/**, Guide]\n@ref[docs/guide.md as Guide]\nSee `docs/guide.md`, src/x.rs, `src/x.rs#run`, src/*, src/, and Guide (@[Guide]).\n",
            ),
            (
                "docs/b.md",
                "@noref[src/]\nSee `docs/guide.md`, src/x.rs, and src/.\n",
            ),
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
        assert_eq!(report.summary.ignored, 7);
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
        let spans: Vec<&str> = sites(&report)
            .iter()
            .map(|(_, c)| &source[c.site.span.start..c.site.span.end])
            .collect();
        assert_eq!(spans, vec!["bar.md", "foo.ts"]);
        assert_eq!(sites(&report)[1].1.site.span.start, 23);
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
    fn one_token_with_one_verdict_is_one_group_and_groups_have_a_fixed_order() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "@ref[docs/guide.md as Guide] @ref[docs/guide.md as Spare]\nSee gen.js, `gen.js`, and `docs/guide.md`; the Guide and `Guide` (@[Guide]).\n",
            ),
            ("docs/b.md", "Header from gen.js.\n"),
            ("docs/guide.md", "# Guide\n"),
            ("src/x.rs", "// Generated by gen.js\n"),
        ]);
        let report = fixture.coverage();
        let described: Vec<(&str, &str, usize)> = report
            .candidates
            .iter()
            .map(|group| {
                let kind = match &group.kind {
                    CandidateKind::Proposal { .. } => "proposal",
                    CandidateKind::Unresolvable { .. } => "unresolvable",
                    CandidateKind::UnusedAlias { .. } => "unused alias",
                    CandidateKind::UnusedIgnore { .. } => "unused ignore",
                };
                (kind, group.token.as_str(), group.sites.len())
            })
            .collect();
        assert_eq!(
            described,
            vec![
                ("proposal", "Guide", 2),
                ("proposal", "docs/guide.md", 1),
                ("unresolvable", "gen.js", 4),
                ("unused alias", "Spare", 1),
            ]
        );
        let generated = &report.candidates[2];
        let paths: Vec<String> = generated
            .sites
            .iter()
            .map(|c| c.site.path.to_string())
            .collect();
        assert_eq!(
            paths,
            vec!["docs/a.md", "docs/a.md", "docs/b.md", "src/x.rs"]
        );
        let texts: Vec<&str> = generated.sites.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["gen.js", "`gen.js`", "gen.js", "gen.js"]);
        assert_eq!(report.summary.unresolvable, 4);
        assert_eq!(report.summary.proposals, 3);
    }

    #[test]
    fn bare_tokens_that_resolve_beside_their_file_are_proposed_with_dot_slash() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "See guide.md, `sub/x.md`, x.rs#run and sub/*.\n",
            ),
            ("docs/guide.md", "# Guide\n"),
            ("docs/sub/x.md", ""),
            ("docs/x.rs", "pub fn run() {}\n"),
        ]);
        assert_eq!(
            describe(&fixture.coverage()),
            vec![
                row("docs/a.md", "guide.md", "propose @ref[./guide.md]"),
                row("docs/a.md", "`sub/x.md`", "propose @ref[./sub/x.md]"),
                row("docs/a.md", "x.rs#run", "propose @ref[./x.rs#run]"),
                row("docs/a.md", "sub/*", "propose @ref[./sub/]"),
            ]
        );
    }

    #[test]
    fn dot_only_tokens_are_prose_about_the_syntax() {
        let fixture = Fixture::new(&[
            (
                "docs/a.md",
                "Write `./` or `../` for relative paths, never `../../`.\n",
            ),
            ("docs/guide.md", ""),
        ]);
        let report = fixture.coverage();
        assert!(report.candidates.is_empty(), "{:?}", describe(&report));
    }

    #[test]
    fn a_token_found_under_exactly_one_ancestor_is_proposed_root_relative() {
        let one = Fixture::new(&[
            ("a/b/c/d.md", "See n/f.md and n/f.md#run.\n"),
            ("a/b/n/f.md", ""),
        ]);
        assert_eq!(
            describe(&one.coverage()),
            vec![
                row("a/b/c/d.md", "n/f.md", "propose @ref[a/b/n/f.md]"),
                row("a/b/c/d.md", "n/f.md#run", "unresolvable"),
            ]
        );

        let two = Fixture::new(&[
            ("a/b/c/d.md", "See n/f.md.\n"),
            ("a/b/n/f.md", ""),
            ("a/n/f.md", ""),
        ]);
        assert_eq!(
            describe(&two.coverage()),
            vec![row("a/b/c/d.md", "n/f.md", "unresolvable")]
        );
    }

    #[test]
    fn an_unresolvable_token_with_a_unique_basename_names_where_it_is() {
        let unique = Fixture::new(&[
            ("docs/a.md", "See missing/guide.md.\n"),
            ("elsewhere/deep/guide.md", ""),
        ]);
        let report = unique.coverage();
        let CandidateKind::Unresolvable { reason } = &report.candidates[0].kind else {
            panic!("{:?}", describe(&report));
        };
        assert!(
            reason.ends_with("; one file named `guide.md` exists at `elsewhere/deep/guide.md`"),
            "{reason}"
        );

        let shared = Fixture::new(&[
            ("docs/a.md", "See missing/guide.md.\n"),
            ("elsewhere/deep/guide.md", ""),
            ("other/guide.md", ""),
        ]);
        let report = shared.coverage();
        let CandidateKind::Unresolvable { reason } = &report.candidates[0].kind else {
            panic!("{:?}", describe(&report));
        };
        assert!(!reason.contains("one file named"), "{reason}");
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
