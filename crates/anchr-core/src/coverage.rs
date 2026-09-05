//! The demoted heuristic scanner. Opt-in markers give soundness on what is annotated and
//! nothing on what is not; `coverage` reports reference-shaped strings that carry no marker,
//! and `annotate` proposes markers only where the target already resolves. It never errors
//! and never writes on its own.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use crate::check::Workspace;
use crate::edit::TextEdit;
use crate::index::Site;
use crate::marker::parse_target;
use crate::resolve::{Resolution, Resolver};
use crate::root::FilePath;
use crate::span::ByteSpan;
use crate::text::{Container, FileAnalyzer, RegionKind};

/// A path-shaped token: something with a `/` in it, or a bare name with a known extension,
/// optionally followed by `#Symbol`.
static PATH_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::expect_used, reason = "the pattern is a literal, checked by tests")]
    Regex::new(
        r"(?x)
        (?: (?:[A-Za-z0-9_.-]+/)+ [A-Za-z0-9_.-]+
          | [A-Za-z0-9_-]+ \. (?:md|markdown|txt|rs|ts|tsx|mts|cts|js|jsx|mjs|cjs|py|pyi|go|toml|json|yaml|yml) \b
        )
        (?: \# [A-Za-z_$][A-Za-z0-9_$]* )?",
    )
    .expect("path token regex is a valid literal")
});

static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(r"^[A-Za-z_$][A-Za-z0-9_$]*$").expect("identifier regex is a valid literal")
});

/// Backtick spans inside a comment, the comment-world equivalent of markdown code spans.
static COMMENT_CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(r"`([^`\n]+)`").expect("code span regex is a valid literal")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// The target resolves; replacing the token with this marker would add a checked
    /// reference without introducing an error.
    Proposal { replacement: String },
    /// Reference-shaped but does not resolve: quite possibly a stale reference in prose.
    Unresolvable { reason: String },
    /// An identifier declared in more than one scanned file; a human must pick.
    Ambiguous { declared_in: Vec<FilePath> },
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
    pub annotated_refs: usize,
    pub proposals: usize,
    pub unresolvable: usize,
    pub ambiguous: usize,
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

/// Scans the current root. `only_files` narrows which files are scanned for candidates; the
/// symbol index used to place identifiers always covers the whole root. A file that cannot be
/// read or analyzed is skipped: coverage informs, it never fails.
pub fn coverage(workspace: &Workspace, only_files: &[FilePath]) -> CoverageReport {
    let (root, index) = workspace.current();
    let mut analyzer = FileAnalyzer::new(&workspace.registry, root.config.scan.parse_budget);
    let symbols = symbol_index(workspace, &mut analyzer);
    let mut resolver = Resolver::new(&workspace.roots, &workspace.registry);

    let mut paths: Vec<&FilePath> = index
        .file_paths()
        .filter(|path| only_files.is_empty() || only_files.contains(path))
        .collect();
    paths.sort();

    let mut candidates = Vec::new();
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

        for region in regions.iter() {
            let Some(text) = source.get(region.span.start..region.span.end) else {
                continue;
            };
            for token in tokens(text, region.kind) {
                let span = token.span.shifted_by(region.span.start);
                if occupied.iter().any(|taken| taken.intersects(span)) {
                    continue;
                }
                let kind = match token.shape {
                    Shape::Path => classify_path(&mut resolver, &root.name, &token.text),
                    Shape::Identifier => classify_identifier(&symbols, &token.text),
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
    }

    let mut summary = CoverageSummary {
        annotated_refs: index.refs().count(),
        ..CoverageSummary::default()
    };
    for candidate in &candidates {
        match candidate.kind {
            CandidateKind::Proposal { .. } => summary.proposals += 1,
            CandidateKind::Unresolvable { .. } => summary.unresolvable += 1,
            CandidateKind::Ambiguous { .. } => summary.ambiguous += 1,
        }
    }
    CoverageReport {
        candidates,
        summary,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Path,
    Identifier,
}

struct Token {
    /// Relative to the region text. For a code span this is the whole span, backticks included.
    span: ByteSpan,
    /// The reference-shaped content without any backticks.
    text: String,
    shape: Shape,
}

fn tokens(text: &str, kind: RegionKind) -> Vec<Token> {
    match kind {
        RegionKind::InlineCode => code_span_token(text, ByteSpan::new(0, text.len()))
            .into_iter()
            .collect(),
        RegionKind::Comment => {
            let mut found = Vec::new();
            let mut covered = Vec::new();
            for captures in COMMENT_CODE_SPAN.captures_iter(text) {
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                covered.push(ByteSpan::from(whole.range()));
                found.extend(code_span_token(
                    whole.as_str(),
                    ByteSpan::from(whole.range()),
                ));
            }
            found.extend(
                path_tokens(text).filter(|token| !covered.iter().any(|c| c.intersects(token.span))),
            );
            found
        }
        RegionKind::Prose | RegionKind::Whole => path_tokens(text).collect(),
    }
}

/// A code span whose entire content is one path or one identifier.
fn code_span_token(span_text: &str, span: ByteSpan) -> Option<Token> {
    let content = span_text.trim_matches('`').trim();
    if content.is_empty() {
        return None;
    }
    if PATH_TOKEN
        .find(content)
        .is_some_and(|m| m.as_str() == content)
        && has_letter(content)
    {
        return Some(Token {
            span,
            text: content.to_owned(),
            shape: Shape::Path,
        });
    }
    if IDENTIFIER.is_match(content) && content.len() >= 3 {
        return Some(Token {
            span,
            text: content.to_owned(),
            shape: Shape::Identifier,
        });
    }
    None
}

fn path_tokens(text: &str) -> impl Iterator<Item = Token> + '_ {
    PATH_TOKEN.find_iter(text).filter_map(move |found| {
        let mut end = found.end();
        while end > found.start() && matches!(text.as_bytes()[end - 1], b'.' | b',' | b';' | b':') {
            end -= 1;
        }
        let token = &text[found.start()..end];
        if !has_letter(token) || is_glued(text, found.start()) || in_url(text, found.start()) {
            return None;
        }
        Some(Token {
            span: ByteSpan::new(found.start(), end),
            text: token.to_owned(),
            shape: Shape::Path,
        })
    })
}

/// File and directory names have letters in them; `v1/2` and `24/7` do not, at least not in
/// the segment that would have to be a file.
fn has_letter(token: &str) -> bool {
    let path_part = token.split('#').next().unwrap_or(token);
    path_part
        .rsplit('/')
        .next()
        .is_some_and(|last| last.chars().any(|c| c.is_ascii_alphabetic()))
}

/// Preceded by a path or word character, so this is the tail of something longer.
fn is_glued(text: &str, start: usize) -> bool {
    text[..start].chars().next_back().is_some_and(|c| {
        c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '@' | '[' | '~')
    })
}

fn in_url(text: &str, start: usize) -> bool {
    text[..start].ends_with("://")
}

fn classify_path(
    resolver: &mut Resolver<'_>,
    current: &crate::root::RootName,
    token: &str,
) -> Option<CandidateKind> {
    let target = parse_target(token).ok()?.target;
    Some(match resolver.resolve(current, &target) {
        Resolution::Resolved => CandidateKind::Proposal {
            replacement: format!("@ref[{token}]"),
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
                };
                (c.site.path.to_string(), c.text.clone(), kind)
            })
            .collect()
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
                (
                    "README.md".to_owned(),
                    "`docs/guide.md`".to_owned(),
                    "propose @ref[docs/guide.md]".to_owned()
                ),
                (
                    "README.md".to_owned(),
                    "docs/guide.md".to_owned(),
                    "propose @ref[docs/guide.md]".to_owned()
                ),
                (
                    "README.md".to_owned(),
                    "docs/missing.md".to_owned(),
                    "unresolvable".to_owned()
                ),
                (
                    "README.md".to_owned(),
                    "anchr.toml".to_owned(),
                    "propose @ref[anchr.toml]".to_owned()
                ),
            ]
        );
        assert_eq!(report.summary.annotated_refs, 1);
        assert_eq!(report.summary.proposals, 3);
        assert_eq!(report.summary.unresolvable, 1);
        assert_eq!(report.summary.total(), 5);
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
                (
                    "docs/a.md".to_owned(),
                    "`validate_token`".to_owned(),
                    "propose @ref[src/auth.rs#validate_token]".to_owned()
                ),
                (
                    "docs/a.md".to_owned(),
                    "`shared_name`".to_owned(),
                    "ambiguous x2".to_owned()
                ),
                (
                    "src/other.rs".to_owned(),
                    "`validate_token`".to_owned(),
                    "propose @ref[src/auth.rs#validate_token]".to_owned()
                ),
                (
                    "src/other.rs".to_owned(),
                    "src/auth.rs".to_owned(),
                    "propose @ref[src/auth.rs]".to_owned()
                ),
            ]
        );
        assert_eq!(report.summary.ambiguous, 1);
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
}
