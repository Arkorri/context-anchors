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

/// A code span whose entire content is one path. A bare identifier is never a candidate: a name
/// has no single referent, so the tool cannot know which declaration the author meant.
fn code_span_token(span_text: &str, span: ByteSpan, known: KnownExtensions<'_>) -> Option<Token> {
    let content = span_text.trim_matches('`').trim();
    let is_whole_path = PATH_TOKEN
        .find(content)
        .is_some_and(|m| m.as_str() == content);
    let shape = is_whole_path
        .then(|| path_shape(content, known))
        .flatten()?;
    Some(Token {
        span,
        text: content.to_owned(),
        shape: Shape::Path(shape),
    })
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
mod tests;
