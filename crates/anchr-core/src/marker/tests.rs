use crate::text::RegionKind;

use super::*;

fn marker(payload: MarkerPayload) -> Marker {
    Marker {
        payload,
        span: ByteSpan::new(0, 1),
        body_span: ByteSpan::new(0, 1),
        region: RegionKind::Prose,
    }
}

fn declared(alias: &str, start: usize, end: usize) -> DeclaredAlias {
    DeclaredAlias {
        alias: Alias::parse(alias).unwrap(),
        span: ByteSpan::new(start, end),
    }
}

#[test]
fn every_payload_reports_its_kind() {
    let cases = [
        (
            marker(MarkerPayload::Anchor {
                id: AnchorId::parse("x").unwrap(),
            }),
            MarkerKind::Anchor,
        ),
        (
            marker(MarkerPayload::Ref {
                target: parse_target("a.md", None).unwrap().target,
                id_span: None,
                alias: None,
            }),
            MarkerKind::Ref,
        ),
        (
            marker(MarkerPayload::Use {
                alias: Alias::parse("Flow").unwrap(),
            }),
            MarkerKind::Use,
        ),
        (
            marker(MarkerPayload::NoRef { items: Vec::new() }),
            MarkerKind::NoRef,
        ),
    ];
    for (marker, expected) in cases {
        assert_eq!(marker.kind(), expected);
    }
}

/// `Use` renders as the syntax rather than the variant name, and it reaches users through
/// `malformed @[...]` diagnostics.
#[test]
fn each_kind_displays_as_the_syntax_the_author_typed() {
    assert_eq!(MarkerKind::Anchor.to_string(), "@anchor");
    assert_eq!(MarkerKind::Ref.to_string(), "@ref");
    assert_eq!(MarkerKind::Use.to_string(), "@[...]");
    assert_eq!(MarkerKind::NoRef.to_string(), "@noref");
}

#[test]
fn shifting_a_declared_alias_moves_its_span_and_leaves_the_name() {
    let shifted = declared("Flow", 4, 8).shifted_by(10);
    assert_eq!(shifted.alias.as_str(), "Flow");
    assert_eq!(shifted.span, ByteSpan::new(14, 18));
}

#[test]
fn shifting_by_zero_changes_nothing_and_shifts_compose() {
    let original = declared("Flow", 4, 8);
    assert_eq!(original.clone().shifted_by(0), original);
    assert_eq!(
        original.clone().shifted_by(3).shifted_by(4),
        original.shifted_by(7)
    );
}
