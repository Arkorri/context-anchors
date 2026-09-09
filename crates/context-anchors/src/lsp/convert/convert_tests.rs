use anchr_core::span::LineIndex;

use super::*;

const SOURCE: &str = "first line\nsecond line\nthird";

fn abs(path: &str) -> Utf8PathBuf {
    if cfg!(windows) {
        Utf8PathBuf::from(format!(r"C:\{}", path.replace('/', r"\")))
    } else {
        Utf8PathBuf::from(format!("/{path}"))
    }
}

#[test]
fn every_offset_round_trips_through_a_protocol_position_under_both_encodings() {
    let index = LineIndex::new(SOURCE).unwrap();
    for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
        for byte in 0..=SOURCE.len() {
            if !SOURCE.is_char_boundary(byte) {
                continue;
            }
            let protocol = index.protocol_position(byte, encoding).unwrap();
            assert_eq!(
                offset(&index, position(protocol), encoding),
                Some(byte),
                "{encoding:?} lost offset {byte}"
            );
        }
    }
}

/// A non-BMP character is 4 UTF-8 bytes but 2 UTF-16 code units, so the two encodings must
/// disagree about the column after it. Negotiating the wrong one silently misplaces every
/// diagnostic on the line.
#[test]
fn a_non_bmp_character_yields_different_columns_per_encoding() {
    let source = "let emoji = \"🎯\"; // after";
    let index = LineIndex::new(source).unwrap();
    let after = source.find("; //").unwrap();

    let utf8 = index
        .protocol_position(after, PositionEncoding::Utf8)
        .unwrap();
    let utf16 = index
        .protocol_position(after, PositionEncoding::Utf16)
        .unwrap();

    assert_eq!(utf8.line, 0);
    assert_eq!(utf16.line, 0);
    assert_ne!(
        utf8.character, utf16.character,
        "a 4-byte / 2-unit char must shift the UTF-8 column past the UTF-16 one"
    );
    assert_eq!(u32::try_from(after).unwrap(), utf8.character);
}

#[test]
fn a_span_reaching_past_the_end_of_the_text_is_none_rather_than_a_panic() {
    let index = LineIndex::new(SOURCE).unwrap();
    let past_end = ByteSpan::new(0, SOURCE.len() + 100);
    assert_eq!(range(&index, past_end, PositionEncoding::Utf8), None);
}

#[test]
fn a_span_inside_the_text_becomes_a_range_spanning_its_lines() {
    let index = LineIndex::new(SOURCE).unwrap();
    let span = ByteSpan::new(0, SOURCE.find("third").unwrap());
    let range = range(&index, span, PositionEncoding::Utf8).unwrap();
    assert_eq!(
        range.start,
        Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        range.end,
        Position {
            line: 2,
            character: 0
        }
    );
}

#[test]
fn a_uri_round_trips_back_to_the_root_relative_path() {
    let root = abs("repo");
    let path = FilePath::new(Utf8PathBuf::from("docs/a.md")).unwrap();
    let uri = uri_for(&root, &path).unwrap();
    assert_eq!(file_path_in(&root, &uri), Some(path));
}

#[test]
fn a_uri_outside_the_root_has_no_root_relative_path() {
    let uri = uri_for(
        &abs("elsewhere"),
        &FilePath::new(Utf8PathBuf::from("a.md")).unwrap(),
    )
    .unwrap();
    assert_eq!(file_path_in(&abs("repo"), &uri), None);
}

#[test]
fn the_root_directory_itself_is_not_a_file_inside_the_root() {
    let root = abs("repo");
    let uri = Uri::from_file_path(root.as_std_path()).unwrap();
    assert_eq!(file_path_in(&root, &uri), None);
}
