use std::sync::LazyLock;

use regex::Regex;

use super::{
    Alias, AnchorId, MalformedMarker, MalformedReason, Marker, MarkerKind, MarkerPayload,
    parse_noref_body, parse_target,
};
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
/// never examined, which is how code fences and non-comment code stay unchecked.
pub fn lex(source: &str, regions: &TextRegions) -> Result<Lexed, LexError> {
    let mut lexed = Lexed::default();
    for region in regions.iter() {
        lex_region(source, region, &mut lexed)?;
    }
    Ok(lexed)
}

fn lex_region(source: &str, region: &TextRegion, lexed: &mut Lexed) -> Result<(), LexError> {
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
                match parse_body(kind, body.as_str(), body_span) {
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
        MarkerKind::Ref => parse_target(body)
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
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::marker::{RefTarget, TargetError};
    use crate::text::RegionKind;

    fn lex_all(source: &str) -> Lexed {
        lex(source, &TextRegions::whole(source.len(), RegionKind::Whole)).unwrap()
    }

    fn slice(source: &str, span: ByteSpan) -> &str {
        &source[span.start..span.end]
    }

    #[test]
    fn finds_anchor_and_ref_with_exact_spans() {
        let source = "## Grammar @anchor[ref-grammar] see @ref[#auth/flow].";
        let lexed = lex_all(source);
        assert!(lexed.malformed.is_empty());
        assert_eq!(lexed.markers.len(), 2);

        let anchor = &lexed.markers[0];
        assert_eq!(slice(source, anchor.span), "@anchor[ref-grammar]");
        assert_eq!(slice(source, anchor.body_span), "ref-grammar");
        assert_eq!(anchor.kind(), MarkerKind::Anchor);

        let reference = &lexed.markers[1];
        assert_eq!(slice(source, reference.span), "@ref[#auth/flow]");
        let MarkerPayload::Ref {
            target, id_span, ..
        } = &reference.payload
        else {
            panic!("expected a ref");
        };
        assert!(matches!(target, RefTarget::Anchor { root: None, .. }));
        assert_eq!(slice(source, id_span.unwrap()), "auth/flow");
    }

    #[test]
    fn only_lexes_inside_the_given_regions() {
        let source = "@ref[a.md] `@ref[b.md]` @ref[c.md]";
        let regions = TextRegions::new(vec![
            TextRegion {
                span: ByteSpan::new(0, 11),
                kind: RegionKind::Prose,
            },
            TextRegion {
                span: ByteSpan::new(23, source.len()),
                kind: RegionKind::Prose,
            },
        ]);
        let lexed = lex(source, &regions).unwrap();
        let bodies: Vec<&str> = lexed
            .markers
            .iter()
            .map(|marker| slice(source, marker.body_span))
            .collect();
        assert_eq!(bodies, vec!["a.md", "c.md"]);
    }

    #[test]
    fn reports_malformed_markers_by_reason() {
        let source = "@ref[ and @anchor[] and @ref[a b] and @anchor[-x]";
        let lexed = lex_all(source);
        assert!(lexed.markers.is_empty());
        let reasons: Vec<&MalformedReason> = lexed.malformed.iter().map(|m| &m.reason).collect();
        assert!(matches!(reasons[0], MalformedReason::Unclosed));
        assert!(matches!(reasons[1], MalformedReason::EmptyBody));
        assert!(matches!(
            reasons[2],
            MalformedReason::InvalidTarget {
                reason: TargetError::BadAliasClause,
                ..
            }
        ));
        assert!(matches!(
            reasons[3],
            MalformedReason::InvalidAnchorId { .. }
        ));
        assert_eq!(slice(source, lexed.malformed[0].span), "@ref[");
    }

    #[test]
    fn an_unclosed_opener_does_not_swallow_the_next_line() {
        let source = "@ref[oops\n@ref[ok.md]";
        let lexed = lex_all(source);
        assert_eq!(lexed.malformed.len(), 1);
        assert_eq!(lexed.markers.len(), 1);
        assert_eq!(slice(source, lexed.markers[0].body_span), "ok.md");
    }

    #[test]
    fn a_marker_glued_to_a_word_is_not_a_marker_regardless_of_script() {
        assert!(lex_all("foo@ref[x.md]").markers.is_empty());
        assert!(lex_all("é@ref[x.md]").markers.is_empty());
        assert!(lex_all("_@ref[x.md]").markers.is_empty());
        assert_eq!(lex_all("(@ref[x.md])").markers.len(), 1);
        assert_eq!(lex_all("«@ref[x.md]»").markers.len(), 1);
    }

    #[test]
    fn either_escape_form_suppresses_the_marker() {
        assert!(lex_all(r"@ref\[x.md\]").markers.is_empty());
        assert_eq!(lex_all(r"@ref\[x.md\]").malformed.len(), 0);
        assert!(lex_all(r"\@ref[x.md]").markers.is_empty());
        assert!(lex_all(r"\@anchor[x]").markers.is_empty());
    }

    #[test]
    fn multibyte_text_before_a_marker_keeps_offsets_exact() {
        let source = "日本語 → @anchor[jp] ← ✓";
        let lexed = lex_all(source);
        assert_eq!(slice(source, lexed.markers[0].span), "@anchor[jp]");
    }

    #[test]
    fn crlf_line_endings_do_not_leak_into_bodies() {
        let source = "@ref[a.md]\r\n@anchor[b]\r\n";
        let lexed = lex_all(source);
        assert_eq!(lexed.markers.len(), 2);
        assert!(lexed.malformed.is_empty());
    }

    #[test]
    fn multiple_markers_on_one_line_are_all_found() {
        let lexed = lex_all("@anchor[a] @anchor[b] @ref[#a] @ref[#b]");
        assert_eq!(lexed.markers.len(), 4);
    }

    #[test]
    fn finds_alias_declarations_and_uses_with_exact_spans() {
        let source = "@ref[#auth/flow as Flow] then @[Flow].";
        let lexed = lex_all(source);
        assert!(lexed.malformed.is_empty());
        assert_eq!(lexed.markers.len(), 2);

        let MarkerPayload::Ref {
            id_span,
            alias: Some(declared),
            ..
        } = &lexed.markers[0].payload
        else {
            panic!("expected an aliased ref");
        };
        assert_eq!(slice(source, id_span.unwrap()), "auth/flow");
        assert_eq!(slice(source, declared.span), "Flow");
        assert_eq!(declared.alias.as_str(), "Flow");

        let use_site = &lexed.markers[1];
        assert_eq!(use_site.kind(), MarkerKind::Use);
        assert_eq!(slice(source, use_site.span), "@[Flow]");
        assert_eq!(slice(source, use_site.body_span), "Flow");
        assert!(
            matches!(&use_site.payload, MarkerPayload::Use { alias } if alias.as_str() == "Flow")
        );
    }

    #[test]
    fn reports_malformed_uses_by_reason() {
        let empty = lex_all("@[]");
        assert_eq!(empty.malformed[0].kind, MarkerKind::Use);
        assert!(matches!(
            empty.malformed[0].reason,
            MalformedReason::EmptyBody
        ));

        let padded = lex_all("@[ X ]");
        assert!(matches!(
            padded.malformed[0].reason,
            MalformedReason::InvalidAlias { .. }
        ));

        let unclosed = lex_all("@[X");
        assert!(matches!(
            unclosed.malformed[0].reason,
            MalformedReason::Unclosed
        ));
        assert_eq!(unclosed.malformed[0].kind, MarkerKind::Use);

        let nested = lex_all("@[@[Y]]");
        assert_eq!(nested.malformed.len(), 1);
        assert!(matches!(
            nested.malformed[0].reason,
            MalformedReason::Unclosed
        ));
        assert_eq!(nested.markers.len(), 1);
        assert_eq!(nested.markers[0].kind(), MarkerKind::Use);
    }

    #[test]
    fn a_use_that_is_glued_escaped_or_misspelled_is_not_a_marker() {
        assert!(lex_all("a@[X]").markers.is_empty());
        assert!(lex_all(r"\@[X]").markers.is_empty());
        assert!(lex_all(r"\@[X]").malformed.is_empty());
        assert_eq!(lex_all("(@[X])").markers.len(), 1);
        for source in [
            "@reference[x.md]",
            "@anchors[x]",
            "@Ref[x.md]",
            "@norefs[x]",
            "@NoRef[x]",
            "a@noref[x]",
            r"\@noref[x]",
        ] {
            let lexed = lex_all(source);
            assert!(lexed.markers.is_empty(), "{source}");
            assert!(lexed.malformed.is_empty(), "{source}");
        }
    }

    #[test]
    fn uses_and_declarations_survive_crlf() {
        let lexed = lex_all("@[A]\r\n@ref[#a as A]\r\n@noref[a, b]\r\n");
        assert_eq!(lexed.markers.len(), 3);
        assert!(lexed.malformed.is_empty());
    }

    #[test]
    fn finds_noref_lists_with_exact_item_spans() {
        let source = "héllo @noref[a, b/, c.ts#Name] and // @noref[x]";
        let lexed = lex_all(source);
        assert!(lexed.malformed.is_empty(), "{lexed:?}");
        assert_eq!(lexed.markers.len(), 2);
        let first = &lexed.markers[0];
        assert_eq!(first.kind(), MarkerKind::NoRef);
        assert_eq!(slice(source, first.span), "@noref[a, b/, c.ts#Name]");
        assert_eq!(slice(source, first.body_span), "a, b/, c.ts#Name");
        let MarkerPayload::NoRef { items } = &first.payload else {
            panic!("expected a noref payload");
        };
        let texts: Vec<&str> = items.iter().map(|item| slice(source, item.span)).collect();
        assert_eq!(texts, vec!["a", "b/", "c.ts#Name"]);
        assert!(
            items
                .iter()
                .all(|item| item.entry.as_str() == slice(source, item.span))
        );
        let MarkerPayload::NoRef { items } = &lexed.markers[1].payload else {
            panic!("expected a noref payload");
        };
        assert_eq!(slice(source, items[0].span), "x");
    }

    #[test]
    fn reports_malformed_noref_lists_by_reason() {
        let empty = lex_all("@noref[]");
        assert_eq!(empty.malformed[0].kind, MarkerKind::NoRef);
        assert!(matches!(
            empty.malformed[0].reason,
            MalformedReason::EmptyBody
        ));
        for source in [
            "@noref[ a]",
            "@noref[a ]",
            "@noref[a,]",
            "@noref[a,,b]",
            "@noref[a b]",
            "@noref[a,`b`]",
        ] {
            let lexed = lex_all(source);
            assert_eq!(lexed.malformed.len(), 1, "{source}");
            assert!(
                matches!(
                    lexed.malformed[0].reason,
                    MalformedReason::InvalidNoRef { .. }
                ),
                "{source}: {:?}",
                lexed.malformed[0].reason
            );
        }
        let unclosed = lex_all("@noref[a, b");
        assert!(matches!(
            unclosed.malformed[0].reason,
            MalformedReason::Unclosed
        ));
        assert_eq!(unclosed.malformed[0].kind, MarkerKind::NoRef);
        let bracket = lex_all("@noref[a[b]");
        assert!(matches!(
            bracket.malformed[0].reason,
            MalformedReason::Unclosed
        ));
    }

    #[test]
    fn a_region_off_a_char_boundary_is_an_error_not_a_panic() {
        let source = "é@ref[x]";
        let regions = TextRegions::new(vec![TextRegion {
            span: ByteSpan::new(1, source.len()),
            kind: RegionKind::Prose,
        }]);
        assert!(matches!(
            lex(source, &regions),
            Err(LexError::RegionNotOnCharBoundary { .. })
        ));
    }

    fn valid_marker() -> impl Strategy<Value = String> {
        prop_oneof![
            "[A-Za-z0-9_][A-Za-z0-9_.-]{0,10}".prop_map(|id| format!("@anchor[{id}]")),
            "[A-Za-z0-9_][A-Za-z0-9_.-]{0,10}".prop_map(|id| format!("@ref[#{id}]")),
            "[a-z]{1,8}(/[a-z]{1,8}){0,3}\\.[a-z]{1,3}".prop_map(|p| format!("@ref[{p}]")),
            "[A-Za-z_][A-Za-z0-9_]{0,10}".prop_map(|alias| format!("@[{alias}]")),
            "[a-z]{1,6}(\\.[a-z]{1,3})?(, ?[a-z]{1,6}(\\.[a-z]{1,3})?){0,2}"
                .prop_map(|list| format!("@noref[{list}]")),
            (
                "[A-Za-z0-9_][A-Za-z0-9_.-]{0,10}",
                "[A-Za-z_][A-Za-z0-9_]{0,10}"
            )
                .prop_map(|(id, alias)| format!("@ref[#{id} as {alias}]")),
        ]
    }

    fn filler() -> impl Strategy<Value = String> {
        "[^@\\[\\]\\\\]{0,12}".prop_filter("must not end in a word char", |s| {
            !s.chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }

    proptest! {
        #[test]
        fn every_embedded_valid_marker_is_found_with_its_exact_span(
            parts in prop::collection::vec((filler(), valid_marker()), 0..6),
            tail in filler(),
        ) {
            let mut source = String::new();
            let mut expected = Vec::new();
            for (before, marker) in &parts {
                source.push_str(before);
                expected.push((source.len(), marker.clone()));
                source.push_str(marker);
            }
            source.push_str(&tail);

            let lexed = lex_all(&source);
            prop_assert!(lexed.malformed.is_empty(), "{lexed:?}");
            prop_assert_eq!(lexed.markers.len(), expected.len());
            for (marker, (start, text)) in lexed.markers.iter().zip(&expected) {
                prop_assert_eq!(marker.span.start, *start);
                prop_assert_eq!(slice(&source, marker.span), text.as_str());
            }
        }

        #[test]
        fn text_without_an_opener_yields_nothing(source in "[^@]{0,64}|@[^arn\\[][^\\[]{0,32}") {
            let lexed = lex_all(&source);
            prop_assert!(lexed.markers.is_empty());
            prop_assert!(lexed.malformed.is_empty());
        }

        #[test]
        fn never_panics_on_arbitrary_input(source in "\\PC{0,80}") {
            let _ = lex_all(&source);
        }
    }
}
