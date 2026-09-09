use super::*;

#[test]
fn edits_apply_in_order_with_length_changes() {
    let source = "@anchor[ab] @ref[#ab] tail";
    let edits = vec![
        TextEdit {
            span: ByteSpan::new(8, 10),
            expected: "ab".into(),
            replacement: "longer-name".into(),
        },
        TextEdit {
            span: ByteSpan::new(18, 20),
            expected: "ab".into(),
            replacement: "longer-name".into(),
        },
    ];
    assert_eq!(
        apply_edits(source, &edits).unwrap(),
        "@anchor[longer-name] @ref[#longer-name] tail"
    );
}

#[test]
fn a_mismatch_or_overlap_refuses_the_whole_file() {
    let source = "@anchor[ab]";
    let stale = vec![TextEdit {
        span: ByteSpan::new(8, 10),
        expected: "zz".into(),
        replacement: "x".into(),
    }];
    assert_eq!(apply_edits(source, &stale), None);
    let overlapping = vec![
        TextEdit {
            span: ByteSpan::new(0, 5),
            expected: "@anch".into(),
            replacement: "".into(),
        },
        TextEdit {
            span: ByteSpan::new(3, 6),
            expected: "cho".into(),
            replacement: "".into(),
        },
    ];
    assert_eq!(apply_edits(source, &overlapping), None);
}
