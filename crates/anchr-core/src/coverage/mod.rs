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
    /// `@noref` entries plus `[ignore] tokens` entries that matched nothing.
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
    /// `[ignore] tokens` entries that matched nothing. Not candidates: they have no site in
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

/// Scans the current root. `only_files` narrows which files are scanned for candidates and
/// counted. A file that cannot be read or analyzed is skipped: coverage informs, it never fails.
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
    let in_scope = |path: &FilePath| only_files.is_empty() || only_files.contains(path);
    let mut global = NoRefSet::new(root.config.ignore.tokens.iter().cloned());

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
mod tests;
