//! Tests of the lexer: each literal kind hides its `//`, each comment kind reports its columns.

use super::*;

/// The `(line, column)` of every reach in `text`.
fn found(text: &str) -> Vec<(u32, usize)> {
    reaches(text)
        .into_iter()
        .map(|reach| (reach.line, reach.column))
        .collect()
}

/// The `(line, column)` of the end of `text`: its line count, and the length of its last line.
/// Where the text ends in a comment, that is the comment's last column.
fn last_comment(text: &str) -> (u32, usize) {
    let line = u32::try_from(text.lines().count()).expect("a few lines");
    let last_line = text.lines().last().expect("a last line");
    let column = last_line.chars().count();
    (line, column)
}

#[test]
fn a_line_comment_reaches_its_last_character() {
    assert_eq!(found("let x = 1; // hi\n"), [(1, 16)]);
}

#[test]
fn doc_comments_are_comments_too() {
    assert_eq!(found("//! a\n/// b\n"), [(1, 5), (2, 5)]);
}

#[test]
fn a_block_comment_reports_each_line_that_holds_text() {
    assert_eq!(found("/* a\n  b */ x\n"), [(1, 4), (2, 6)]);
}

#[test]
fn block_comments_nest() {
    assert_eq!(found("/* a /* b */ c */ x\n"), [(1, 17)]);
}

#[test]
fn an_empty_block_comment_is_a_comment() {
    assert_eq!(found("/**/ x\n"), [(1, 4)]);
}

#[test]
fn a_block_comment_that_never_closes_runs_to_the_end() {
    assert_eq!(found("/* open\nstill open"), [(1, 7), (2, 10)]);
}

#[test]
fn slash_star_slash_does_not_close_a_block_comment() {
    assert_eq!(found("/*/ x */\n"), [(1, 8)]);
}

#[test]
fn a_double_quoted_string_hides_its_slashes_and_escaped_quotes() {
    assert_eq!(found("let s = \"// not a comment\";\n"), []);
    assert_eq!(found("let s = \"a \\\" // still a string\";\n"), []);
    assert_eq!(found("let s = \"a\\\\\"; // real\n"), [(1, 22)]);
}

#[test]
fn a_string_across_lines_keeps_the_line_numbers() {
    assert_eq!(found("let s = \"a\nb\"; // c\n"), [(2, 8)]);
}

#[test]
fn a_raw_string_hides_slashes_whatever_its_hash_count() {
    assert_eq!(found("let s = r\"//\";\n"), []);
    assert_eq!(found("let s = r#\"a \" // \"#;\n"), []);
    let text = "let s = r##\"a \"# // \"##; // real\n";
    assert_eq!(found(text), [(1, last_comment(text).1)]);
}

#[test]
fn a_raw_string_may_hold_newlines() {
    let text = "let s = r#\"\n// line\n\"#; // real\n";
    assert_eq!(found(text), [(3, last_comment(text).1)]);
}

#[test]
fn byte_c_and_raw_prefixed_strings_hide_their_slashes() {
    assert_eq!(found("b\"//\"; c\"//\"; br\"//\"; cr#\"//\"#;\n"), []);
}

#[test]
fn a_raw_identifier_is_not_a_raw_string() {
    assert_eq!(found("let r#type = 1; // c\n"), [(1, 20)]);
}

#[test]
fn char_literals_hide_their_quotes_and_slashes() {
    let text = "let q = ['\"', '\\'', '\\\\', '/', b'/', b'\\''];  // c\n";
    assert_eq!(found(text), [(1, last_comment(text).1)]);
}

#[test]
fn a_char_quote_does_not_open_a_string() {
    let text = "let q = '\"';\nlet s = \"//\"; // c\n";
    assert_eq!(found(text), [(2, last_comment(text).1)]);
}

#[test]
fn lifetimes_and_labels_are_not_char_literals() {
    let text = "fn f<'a, 'static>(x: &'a str) { 'outer: loop { break 'outer; } } // c\n";
    assert_eq!(found(text), [(1, last_comment(text).1)]);
}

#[test]
fn a_unicode_char_literal_is_one_character() {
    let text = "let c = 'é'; let s = \"//\"; // c\n";
    assert_eq!(found(text), [(1, last_comment(text).1)]);
}

#[test]
fn columns_count_characters_not_bytes() {
    assert_eq!(found(&format!("//{}\n", "é".repeat(98))), [(1, 100)]);
}

#[test]
fn crlf_line_endings_do_not_count_as_comment_text() {
    assert_eq!(found("// a\r\n// b\r\n"), [(1, 4), (2, 4)]);
}

#[test]
fn an_unclosed_string_runs_to_the_end_without_a_comment() {
    assert_eq!(found("let s = \"abc // x\n"), []);
}
