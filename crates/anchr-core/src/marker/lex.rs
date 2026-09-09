use std::sync::LazyLock;

use regex::Regex;

use super::{
    Alias, AnchorId, MalformedMarker, MalformedReason, Marker, MarkerKind, MarkerPayload,
    parse_noref_body, parse_target,
};
use crate::root::FilePath;
use crate::span::ByteSpan;
use crate::text::{TextRegion, TextRegions};

/// Group 1: the kind, empty for an alias use `@[...]`. Group 2: the body, absent when the
/// opener has no `]` on its line. `[` is excluded from the body so one unclosed opener cannot
/// swallow the next marker.
static MARKER: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "the pattern is a literal, checked by tests"
    )]
    Regex::new(r"@(anchor|ref|noref|)\[(?:([^\[\]\n]*)\]|)")
        .expect("marker regex is a valid literal")
});

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lexed {
    pub markers: Vec<Marker>,
    pub malformed: Vec<MalformedMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LexError {
    #[error("text region {span:?} does not lie on character boundaries of the source")]
    RegionNotOnCharBoundary { span: ByteSpan },
}

/// Finds every marker inside the given regions of `source`. Bytes outside the regions are
/// never examined, which is how code fences and non-comment code stay unchecked. `written_in`
/// is the file being lexed; `./` and `../` targets are anchored to its directory here, so no
/// marker ever carries a path that is not root-relative.
pub fn lex(source: &str, regions: &TextRegions, written_in: &FilePath) -> Result<Lexed, LexError> {
    let mut lexed = Lexed::default();
    for region in regions.iter() {
        lex_region(source, region, written_in, &mut lexed)?;
    }
    Ok(lexed)
}

fn lex_region(
    source: &str,
    region: &TextRegion,
    written_in: &FilePath,
    lexed: &mut Lexed,
) -> Result<(), LexError> {
    let span = region.span;
    let text = source
        .get(span.start..span.end)
        .ok_or(LexError::RegionNotOnCharBoundary { span })?;

    for captures in MARKER.captures_iter(text) {
        let whole = captures
            .get(0)
            .map(|m| ByteSpan::from(m.range()).shifted_by(span.start));
        let Some(whole) = whole else { continue };
        if is_glued_or_escaped(source, whole.start) {
            continue;
        }
        let kind = match captures.get(1).map(|m| m.as_str()) {
            Some("anchor") => MarkerKind::Anchor,
            Some("ref") => MarkerKind::Ref,
            Some("noref") => MarkerKind::NoRef,
            _ => MarkerKind::Use,
        };
        match captures.get(2) {
            None => lexed.malformed.push(MalformedMarker {
                kind,
                reason: MalformedReason::Unclosed,
                span: whole,
                region: region.kind,
            }),
            Some(body) => {
                let body_span = ByteSpan::from(body.range()).shifted_by(span.start);
                match parse_body(kind, body.as_str(), body_span, written_in) {
                    Ok(payload) => lexed.markers.push(Marker {
                        payload,
                        span: whole,
                        body_span,
                        region: region.kind,
                    }),
                    Err(reason) => lexed.malformed.push(MalformedMarker {
                        kind,
                        reason,
                        span: whole,
                        region: region.kind,
                    }),
                }
            }
        }
    }
    Ok(())
}

/// `foo@ref[x]` inside an email-like token is not a marker, and `\@ref[x]` is an escaped
/// example. Checks the preceding character, not byte, so multi-byte letters behave like
/// ASCII ones.
fn is_glued_or_escaped(source: &str, start: usize) -> bool {
    source[..start]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '\\')
}

fn parse_body(
    kind: MarkerKind,
    body: &str,
    body_span: ByteSpan,
    written_in: &FilePath,
) -> Result<MarkerPayload, MalformedReason> {
    if body.is_empty() {
        return Err(MalformedReason::EmptyBody);
    }
    match kind {
        MarkerKind::Anchor => AnchorId::parse(body)
            .map(|id| MarkerPayload::Anchor { id })
            .map_err(|reason| MalformedReason::InvalidAnchorId {
                raw: body.to_owned(),
                reason,
            }),
        MarkerKind::Ref => parse_target(body, Some(written_in))
            .map(|parsed| MarkerPayload::Ref {
                target: parsed.target,
                id_span: parsed
                    .id_span
                    .map(|relative| relative.shifted_by(body_span.start)),
                alias: parsed
                    .alias
                    .map(|declared| declared.shifted_by(body_span.start)),
            })
            .map_err(|reason| MalformedReason::InvalidTarget {
                raw: body.to_owned(),
                reason,
            }),
        MarkerKind::Use => Alias::parse(body)
            .map(|alias| MarkerPayload::Use { alias })
            .map_err(|reason| MalformedReason::InvalidAlias {
                raw: body.to_owned(),
                reason,
            }),
        MarkerKind::NoRef => parse_noref_body(body)
            .map(|items| MarkerPayload::NoRef {
                items: items
                    .into_iter()
                    .map(|item| item.shifted_by(body_span.start))
                    .collect(),
            })
            .map_err(|reason| MalformedReason::InvalidNoRef {
                raw: body.to_owned(),
                reason,
            }),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
