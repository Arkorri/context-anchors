use super::*;

#[test]
fn only_one_is_singular() {
    assert_eq!(plural(0), "s");
    assert_eq!(plural(1), "");
    assert_eq!(plural(2), "s");
    assert_eq!(plural(usize::MAX), "s");
}
