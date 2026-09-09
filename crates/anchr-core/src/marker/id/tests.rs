use super::*;

#[test]
fn accepts_flat_and_hierarchical_ids() {
    assert_eq!(
        AnchorId::parse("ref-grammar").unwrap().as_str(),
        "ref-grammar"
    );
    let id = AnchorId::parse("auth/token-refresh.v2").unwrap();
    assert_eq!(id.last_segment(), "token-refresh.v2");
}

#[test]
fn rejects_each_malformed_shape() {
    assert_eq!(AnchorId::parse(""), Err(IdError::Empty));
    assert_eq!(AnchorId::parse("a//b"), Err(IdError::EmptySegment));
    assert_eq!(AnchorId::parse("/a"), Err(IdError::EmptySegment));
    assert_eq!(AnchorId::parse("a/"), Err(IdError::EmptySegment));
    assert_eq!(
        AnchorId::parse("-a"),
        Err(IdError::InvalidSegmentStart { ch: '-' })
    );
    assert_eq!(
        AnchorId::parse(".a"),
        Err(IdError::InvalidSegmentStart { ch: '.' })
    );
    assert_eq!(
        AnchorId::parse("a b"),
        Err(IdError::InvalidChar { ch: ' ' })
    );
    assert_eq!(
        AnchorId::parse("a:b"),
        Err(IdError::InvalidChar { ch: ':' })
    );
    assert_eq!(
        AnchorId::parse("é"),
        Err(IdError::InvalidSegmentStart { ch: 'é' })
    );
}

#[test]
fn enforces_length_limits() {
    let long_segment = "a".repeat(MAX_SEGMENT_BYTES + 1);
    assert!(matches!(
        AnchorId::parse(&long_segment),
        Err(IdError::SegmentTooLong { .. })
    ));
    let long_id = vec!["a".repeat(MAX_SEGMENT_BYTES); 5].join("/");
    assert!(matches!(
        AnchorId::parse(&long_id),
        Err(IdError::TooLong { .. })
    ));
}
