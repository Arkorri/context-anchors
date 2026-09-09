use super::*;

#[test]
fn spans_intersect_when_they_overlap_by_at_least_one_byte() {
    let a = ByteSpan::new(0, 5);
    assert!(a.intersects(ByteSpan::new(4, 10)));
    assert!(!a.intersects(ByteSpan::new(5, 10)));
    assert!(!ByteSpan::new(5, 10).intersects(a));
}

#[test]
fn line_col_is_one_based_and_counts_utf8_bytes() {
    let index = LineIndex::new("ab\né@x").unwrap();
    assert_eq!(index.line_col(0).unwrap(), LineCol { line: 1, col: 1 });
    assert_eq!(index.line_col(3).unwrap(), LineCol { line: 2, col: 1 });
    // `é` is two bytes, so `@` sits at byte column 3.
    assert_eq!(index.line_col(5).unwrap(), LineCol { line: 2, col: 3 });
}

#[test]
fn end_of_text_is_a_valid_position_but_beyond_it_is_not() {
    let index = LineIndex::new("abc").unwrap();
    assert_eq!(index.line_col(3).unwrap(), LineCol { line: 1, col: 4 });
    assert_eq!(index.line_col(4), Err(PositionOverflow { offset: 4 }));
}

#[test]
fn crlf_leaves_the_carriage_return_on_the_previous_line() {
    let index = LineIndex::new("a\r\nb").unwrap();
    assert_eq!(index.line_col(3).unwrap(), LineCol { line: 2, col: 1 });
}

#[test]
fn protocol_positions_round_trip_in_both_encodings() {
    // `é` is 2 UTF-8 bytes and 1 UTF-16 unit; `😀` is 4 bytes and 2 units.
    let text = "ab\né😀@x";
    let index = LineIndex::new(text).unwrap();
    let at = text.find('@').unwrap();

    let utf8 = index.protocol_position(at, PositionEncoding::Utf8).unwrap();
    assert_eq!((utf8.line, utf8.character), (1, 6));
    assert_eq!(index.offset_of(utf8, PositionEncoding::Utf8), Some(at));

    let utf16 = index
        .protocol_position(at, PositionEncoding::Utf16)
        .unwrap();
    assert_eq!((utf16.line, utf16.character), (1, 3));
    assert_eq!(index.offset_of(utf16, PositionEncoding::Utf16), Some(at));

    let beyond = ProtocolPosition {
        line: 9,
        character: 0,
    };
    assert_eq!(index.offset_of(beyond, PositionEncoding::Utf16), None);
}
