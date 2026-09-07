//! Reference-shaped tokens: what coverage looks for in prose, code spans, and comments before
//! asking whether any of it resolves.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::linguist::is_linguist_extension;
use crate::marker::Alias;
use crate::span::ByteSpan;
use crate::text::RegionKind;

/// Path characters in `/`-separated segments, optionally ending in a directory tail (`/`, `/*`,
/// `/**`) or a `#Symbol`. Matching is structural; [`path_shape`] decides whether a match is a
/// candidate.
static PATH_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(
        r"(?x)
        [A-Za-z0-9_.-]+ (?: / [A-Za-z0-9_.-]+ )*
        (?: / \*{0,2} | \# [A-Za-z_$][A-Za-z0-9_$]* )?",
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

/// An identifier-shaped word; matched against the file's declared aliases.
static WORD: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("word regex is a valid literal")
});

/// Backtick spans inside a comment, the comment-world equivalent of markdown code spans.
static COMMENT_CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(r"`([^`\n]+)`").expect("code span regex is a valid literal")
});

/// Extensions a `stem.ext` token may carry: GitHub Linguist's list plus every extension the scan
/// walked past in this root. The union keeps a `.txt` mention a candidate after the last such
/// file is renamed, and keeps an in-house format a candidate as long as one such file exists.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KnownExtensions<'a> {
    repo: &'a BTreeSet<String>,
}

impl<'a> KnownExtensions<'a> {
    pub(crate) fn new(repo: &'a BTreeSet<String>) -> Self {
        Self { repo }
    }

    fn contains(self, extension: &str) -> bool {
        let lower = extension.to_ascii_lowercase();
        is_linguist_extension(&lower) || self.repo.contains(&lower)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathShape {
    /// The last segment is `stem.ext` with a known extension, optionally followed by `#Symbol`.
    File,
    /// Ends in `/`.
    Directory,
    /// Ends in `/*` or `/**`; proposed as the directory it globs.
    Glob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    Path(PathShape),
    Identifier,
    /// A word equal to an alias this file declares: the highest-confidence candidate there is.
    Alias(Alias),
}

pub(crate) struct Token {
    /// Relative to the region text. For a code span this is the whole span, backticks included.
    pub(crate) span: ByteSpan,
    /// The reference-shaped content without any backticks.
    pub(crate) text: String,
    pub(crate) shape: Shape,
}

/// Alias matches come first and shadow any other token on the same bytes: a code span holding
/// `Analyzer` is a use of this file's alias before it is a symbol declared somewhere.
pub(crate) fn tokens(
    text: &str,
    kind: RegionKind,
    aliases: &[&Alias],
    known: KnownExtensions<'_>,
) -> Vec<Token> {
    let mut found = alias_tokens(text, kind, aliases);
    let taken: Vec<ByteSpan> = found.iter().map(|token| token.span).collect();
    let others: Vec<Token> = match kind {
        RegionKind::InlineCode => code_span_token(text, ByteSpan::new(0, text.len()), known)
            .into_iter()
            .collect(),
        RegionKind::Comment => {
            let mut tokens = Vec::new();
            let mut covered = Vec::new();
            for captures in COMMENT_CODE_SPAN.captures_iter(text) {
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                covered.push(ByteSpan::from(whole.range()));
                tokens.extend(code_span_token(
                    whole.as_str(),
                    ByteSpan::from(whole.range()),
                    known,
                ));
            }
            tokens.extend(
                path_tokens(text, known)
                    .filter(|token| !covered.iter().any(|c| c.intersects(token.span))),
            );
            tokens
        }
        RegionKind::Prose | RegionKind::Whole => path_tokens(text, known).collect(),
    };
    found.extend(
        others
            .into_iter()
            .filter(|token| !taken.iter().any(|span| span.intersects(token.span))),
    );
    found
}

/// Exact, case-sensitive matches against the file's own aliases: a whole code span holding
/// one, or a bare word outside code spans and link text.
fn alias_tokens(text: &str, kind: RegionKind, aliases: &[&Alias]) -> Vec<Token> {
    if aliases.is_empty() {
        return Vec::new();
    }
    let declared = |word: &str| aliases.iter().find(|alias| alias.as_str() == word).copied();
    let alias_token = |span: ByteSpan, alias: &Alias| Token {
        span,
        text: alias.to_string(),
        shape: Shape::Alias(alias.clone()),
    };
    match kind {
        RegionKind::InlineCode => declared(text.trim_matches('`').trim())
            .map(|alias| alias_token(ByteSpan::new(0, text.len()), alias))
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
                if let Some(alias) = declared(whole.as_str().trim_matches('`').trim()) {
                    found.push(alias_token(ByteSpan::from(whole.range()), alias));
                }
            }
            found.extend(
                alias_words(text, &declared)
                    .filter(|token| !covered.iter().any(|c| c.intersects(token.span))),
            );
            found
        }
        RegionKind::Prose | RegionKind::Whole => alias_words(text, &declared).collect(),
    }
}

fn alias_words<'t>(
    text: &'t str,
    declared: &'t dyn Fn(&str) -> Option<&'t Alias>,
) -> impl Iterator<Item = Token> + 't {
    WORD.find_iter(text).filter_map(move |word| {
        if is_glued(text, word.start()) {
            return None;
        }
        let alias = declared(word.as_str())?;
        Some(Token {
            span: ByteSpan::from(word.range()),
            text: alias.to_string(),
            shape: Shape::Alias(alias.clone()),
        })
    })
}

/// A code span whose entire content is one path or one identifier.
fn code_span_token(span_text: &str, span: ByteSpan, known: KnownExtensions<'_>) -> Option<Token> {
    let content = span_text.trim_matches('`').trim();
    if content.is_empty() {
        return None;
    }
    if PATH_TOKEN
        .find(content)
        .is_some_and(|m| m.as_str() == content)
        && let Some(shape) = path_shape(content, known)
    {
        return Some(Token {
            span,
            text: content.to_owned(),
            shape: Shape::Path(shape),
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

fn path_tokens<'t>(text: &'t str, known: KnownExtensions<'t>) -> impl Iterator<Item = Token> + 't {
    PATH_TOKEN.find_iter(text).filter_map(move |found| {
        let mut end = found.end();
        while end > found.start() && matches!(text.as_bytes()[end - 1], b'.' | b',' | b';' | b':') {
            end -= 1;
        }
        let token = &text[found.start()..end];
        if is_glued(text, found.start())
            || in_url(text, found.start())
            || is_glob_pattern(text, found.start(), end)
        {
            return None;
        }
        let shape = path_shape(token, known)?;
        Some(Token {
            span: ByteSpan::new(found.start(), end),
            text: token.to_owned(),
            shape: Shape::Path(shape),
        })
    })
}

/// Whether a token is a candidate, and as what. Directories announce themselves with a tail;
/// files need an extension somebody has actually used. `line/col`, `Apache-2.0/MIT`, `e.g`, and
/// `1.x` all fall out here.
pub(crate) fn path_shape(token: &str, known: KnownExtensions<'_>) -> Option<PathShape> {
    if token.ends_with("/*") || token.ends_with("/**") {
        return Some(PathShape::Glob);
    }
    if token.ends_with('/') {
        return Some(PathShape::Directory);
    }
    let path = token.split_once('#').map_or(token, |(path, _)| path);
    let last = path.rsplit('/').next().unwrap_or(path);
    let (stem, extension) = last.rsplit_once('.')?;
    if !stem.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if last.split('.').all(|piece| piece.chars().count() == 1) {
        return None;
    }
    known.contains(extension).then_some(PathShape::File)
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

/// Part of a glob rather than a reference: `*.generated.ts`, or a `/*` tail that keeps
/// going (`src/*.rs`, `src/**/*.md`). A `*` before a letter is emphasis and stays.
fn is_glob_pattern(text: &str, start: usize, end: usize) -> bool {
    if text[..start].ends_with('*') && text[start..].starts_with('.') {
        return true;
    }
    if !text[..end].ends_with('*') {
        return false;
    }
    let mut rest = text[end..].chars();
    match rest.next() {
        Some('/' | '*') => true,
        Some('.') => rest.next().is_some_and(is_path_char),
        Some(c) => is_path_char(c),
        None => false,
    }
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '*')
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn known(repo: &[&str]) -> BTreeSet<String> {
        repo.iter().map(|e| (*e).to_owned()).collect()
    }

    fn shape(token: &str, repo: &BTreeSet<String>) -> Option<PathShape> {
        path_shape(token, KnownExtensions::new(repo))
    }

    fn prose_tokens(text: &str, repo: &BTreeSet<String>) -> Vec<(String, PathShape)> {
        path_tokens(text, KnownExtensions::new(repo))
            .map(|token| match token.shape {
                Shape::Path(shape) => (token.text, shape),
                Shape::Identifier | Shape::Alias(_) => unreachable!("path tokens only"),
            })
            .collect()
    }

    #[test]
    fn path_shape_classifies_files_directories_and_globs() {
        let repo = known(&[]);
        assert_eq!(shape("file.txt", &repo), Some(PathShape::File));
        assert_eq!(
            shape("src/directory/file.txt", &repo),
            Some(PathShape::File)
        );
        assert_eq!(shape("src/x.rs#run", &repo), Some(PathShape::File));
        assert_eq!(shape("src/directory/", &repo), Some(PathShape::Directory));
        assert_eq!(shape("src/", &repo), Some(PathShape::Directory));
        assert_eq!(shape("src/*", &repo), Some(PathShape::Glob));
        assert_eq!(shape("src/**", &repo), Some(PathShape::Glob));
    }

    #[test]
    fn path_shape_rejects_extensionless_tokens() {
        let repo = known(&[]);
        for token in [
            "src/directory",
            "file",
            "line/col",
            "Apache-2.0/MIT",
            "TS/TSX/JS/Py/Go",
            "pulldown-cmark/pulldown-cmark",
        ] {
            assert_eq!(shape(token, &repo), None, "{token}");
        }
    }

    #[test]
    fn path_shape_requires_a_lettered_stem() {
        let repo = known(&[]);
        for token in ["1.x", "0.13.4", "v1.1", "Apache-2.0"] {
            assert_eq!(shape(token, &repo), None, "{token}");
        }
        assert_eq!(shape("main.c", &repo), Some(PathShape::File));
        assert_eq!(shape("x.rs", &repo), Some(PathShape::File));
    }

    #[test]
    fn path_shape_rejects_all_single_char_pieces() {
        let repo = known(&[]);
        for token in ["e.g", "i.e", "a.k.a", "p.s"] {
            assert_eq!(shape(token, &repo), None, "{token}");
        }
        assert_eq!(shape("a.k.md", &repo), Some(PathShape::File));
    }

    #[test]
    fn path_shape_uses_linguist_then_repo_extensions() {
        assert_eq!(shape("foo.zzq", &known(&[])), None);
        assert_eq!(shape("foo.zzq", &known(&["zzq"])), Some(PathShape::File));
        assert_eq!(shape("README.TXT", &known(&[])), Some(PathShape::File));
        // `com` is a Linguist extension (DCL), so domains stay candidates; the author ignores them.
        assert_eq!(shape("website.com", &known(&[])), Some(PathShape::File));
    }

    #[test]
    fn bare_dotfiles_are_not_candidates_but_dot_directories_are() {
        let repo = known(&[]);
        assert_eq!(shape(".env", &repo), None);
        assert_eq!(shape(".anchrignore", &repo), None);
        assert_eq!(
            shape(".github/workflows/ci.yml", &repo),
            Some(PathShape::File)
        );
    }

    #[test]
    fn path_tokens_keep_directory_tails_and_trim_sentence_punctuation() {
        let repo = known(&[]);
        let text = "in docs/guide.md, docs/, and src/*. Then src/**: done.";
        assert_eq!(
            prose_tokens(text, &repo),
            vec![
                ("docs/guide.md".to_owned(), PathShape::File),
                ("docs/".to_owned(), PathShape::Directory),
                ("src/*".to_owned(), PathShape::Glob),
                ("src/**".to_owned(), PathShape::Glob),
            ]
        );
        let spans: Vec<&str> = path_tokens(text, KnownExtensions::new(&repo))
            .map(|token| &text[token.span.start..token.span.end])
            .collect();
        assert_eq!(spans, vec!["docs/guide.md", "docs/", "src/*", "src/**"]);
    }

    #[test]
    fn path_tokens_skip_glob_patterns() {
        let repo = known(&[]);
        for text in [
            "*.generated.ts",
            "**/*.md",
            "src/*.rs",
            "src/**/*.rs",
            "src/*x",
        ] {
            assert!(prose_tokens(text, &repo).is_empty(), "{text}");
        }
    }

    #[test]
    fn path_tokens_keep_emphasised_paths() {
        let repo = known(&[]);
        assert_eq!(
            prose_tokens("read *docs/guide.md* first", &repo),
            vec![("docs/guide.md".to_owned(), PathShape::File)]
        );
    }

    #[test]
    fn path_tokens_skip_url_paths_and_glued_tails() {
        let repo = known(&[]);
        assert!(prose_tokens("see https://example.com/docs/guide.md", &repo).is_empty());
        assert!(prose_tokens("v1/2 and 24/7", &repo).is_empty());
        assert!(prose_tokens("@ref[docs/guide.md]", &repo).is_empty());
    }

    #[test]
    fn code_span_token_requires_the_whole_span_to_be_one_path() {
        let repo = known(&[]);
        let span_shape = |content: &str| {
            code_span_token(
                content,
                ByteSpan::new(0, content.len()),
                KnownExtensions::new(&repo),
            )
            .map(|token| token.shape)
        };
        assert_eq!(
            span_shape("`docs/research/`"),
            Some(Shape::Path(PathShape::Directory))
        );
        assert_eq!(span_shape("`src/*`"), Some(Shape::Path(PathShape::Glob)));
        assert_eq!(span_shape("`x.rs`"), Some(Shape::Path(PathShape::File)));
        assert_eq!(span_shape("`tests/fixtures/<case>/`"), None);
        assert_eq!(span_shape("`*.generated.ts`"), None);
        assert_eq!(span_shape("`My Notes.md`"), None);
        assert_eq!(span_shape("`e.g`"), None);
        assert_eq!(span_shape("`run_check`"), Some(Shape::Identifier));
        assert_eq!(span_shape("`ab`"), None);
    }

    #[test]
    fn linguist_table_is_sorted_unique_and_lowercase() {
        let table = super::super::linguist::LINGUIST_EXTENSIONS;
        assert!(table.is_sorted());
        assert!(table.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(table.iter().all(|entry| {
            !entry.is_empty()
                && entry.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-')
                })
        }));
        for extension in ["rs", "md", "toml", "txt", "yml", "go", "py"] {
            assert!(is_linguist_extension(extension), "{extension}");
        }
        for extension in ["", "RS", "1", "lock", "rs.in"] {
            assert!(!is_linguist_extension(extension), "{extension}");
        }
    }
}
