//! Closed at compile time (research 37 §7.1): a payload struct with a `String` field does not
//! compile, and no other crate can implement `Closed`.

#[test]
fn a_free_text_field_does_not_compile() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/closed/*.rs");
}
