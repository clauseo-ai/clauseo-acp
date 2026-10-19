//! Tests of the block-quote and list-item prefixes, and of each line's indent inside them.

use super::*;

/// The `(indent, body)` that each line of `lines` reads as, `None` for a blank or marker-only line.
fn read(lines: &[&str]) -> Vec<Option<(usize, String)>> {
    let mut containers = Containers::default();
    lines
        .iter()
        .map(|line| {
            containers
                .strip(line)
                .map(|content| (content.indent, content.body.to_owned()))
        })
        .collect()
}

/// The `(indent, body)` of one line read on its own.
fn one(line: &str) -> Option<(usize, String)> {
    read(&[line]).remove(0)
}

#[test]
fn a_list_item_reads_as_its_text() {
    assert_eq!(one("- item"), Some((0, "item".to_owned())));
    assert_eq!(one("  * item"), Some((0, "item".to_owned())));
    assert_eq!(one("1. one"), Some((0, "one".to_owned())));
    assert_eq!(one("12) no"), Some((0, "no".to_owned())));
}

#[test]
fn a_marker_needs_a_space_after_it_and_at_most_nine_digits() {
    assert_eq!(one("-x"), Some((0, "-x".to_owned())));
    assert_eq!(one("1.five"), Some((0, "1.five".to_owned())));
    assert_eq!(one("1234567890. x"), Some((0, "1234567890. x".to_owned())));
    assert_eq!(one("---"), Some((0, "---".to_owned())));
}

#[test]
fn five_spaces_after_a_marker_make_the_text_an_indented_one() {
    assert_eq!(one("-     code"), Some((4, "code".to_owned())));
}

#[test]
fn a_marker_only_line_is_no_text_but_opens_its_item() {
    let mut containers = Containers::default();
    assert!(containers.strip("-").is_none());
    let next = containers.strip("  b").map(|c| (c.indent, c.body));
    assert_eq!(next, Some((0, "b")));
}

#[test]
fn a_line_indented_into_an_item_is_measured_from_its_text() {
    assert_eq!(read(&["- a", "", "  b"])[2], Some((0, "b".to_owned())));
    assert_eq!(read(&["- a", "", "      b"])[2], Some((4, "b".to_owned())));
}

#[test]
fn an_unindented_line_after_a_blank_closes_the_item() {
    let lines = read(&["- a", "", "b"]);
    assert_eq!(lines[2], Some((0, "b".to_owned())));
}

#[test]
fn a_lazy_paragraph_line_stays_in_the_item() {
    assert_eq!(read(&["- a", "b"])[1], Some((0, "b".to_owned())));
    let mut containers = Containers::default();
    containers.strip("- a");
    containers.strip("b");
    assert_eq!(containers.items, [2]);
}

#[test]
fn a_marker_four_spaces_in_is_text_not_an_item() {
    assert_eq!(one("    - x"), Some((4, "- x".to_owned())));
    assert_eq!(
        read(&["- a", "", "      - x"])[2],
        Some((4, "- x".to_owned()))
    );
    assert_eq!(read(&["- a", "", "    - x"])[2], Some((0, "x".to_owned())));
}

#[test]
fn block_quote_markers_are_stripped_at_any_depth_but_not_under_four_spaces() {
    assert_eq!(one("> - x"), Some((0, "x".to_owned())));
    assert_eq!(one("> > y"), Some((0, "y".to_owned())));
    assert_eq!(one(">z"), Some((0, "z".to_owned())));
    assert_eq!(one("    > x"), Some((4, "> x".to_owned())));
}

#[test]
fn a_quote_line_with_nothing_after_its_marker_is_blank() {
    assert_eq!(one(">"), None);
    assert_eq!(one("> "), None);
}
