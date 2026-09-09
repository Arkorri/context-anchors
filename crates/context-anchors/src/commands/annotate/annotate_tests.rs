use super::*;

fn footer(count: usize, wrote: bool) -> String {
    let mut out = Vec::new();
    write_footer(&mut out, count, wrote).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_preview_tells_the_reader_how_to_apply_it() {
    assert_eq!(footer(0, false), "0 proposals; pass --write to apply\n");
    assert_eq!(footer(1, false), "1 proposal; pass --write to apply\n");
    assert_eq!(footer(4, false), "4 proposals; pass --write to apply\n");
}

#[test]
fn a_write_reports_what_it_did_and_what_to_run_next() {
    assert_eq!(
        footer(1, true),
        "annotated 1 reference; run `anchr check` to confirm\n"
    );
    assert_eq!(
        footer(3, true),
        "annotated 3 references; run `anchr check` to confirm\n"
    );
}

#[test]
fn the_two_footers_never_read_the_same() {
    for count in [0, 1, 2] {
        assert_ne!(footer(count, true), footer(count, false));
    }
}
