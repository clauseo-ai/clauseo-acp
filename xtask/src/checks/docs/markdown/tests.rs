//! Tests of the Markdown reader: fences, code spans, links and reference definitions, with lines.

use super::*;

/// The spans of `text` as `(line, content)`.
fn spans(text: &str) -> Vec<(u32, String)> {
    scan(text)
        .spans
        .into_iter()
        .map(|span| (span.line, span.text))
        .collect()
}

/// The link destinations of `text` as `(line, target)`.
fn targets(text: &str) -> Vec<(u32, String)> {
    scan(text)
        .links
        .into_iter()
        .map(|link| (link.line, link.target))
        .collect()
}

#[test]
fn spans_and_links_carry_their_line_numbers() {
    assert_eq!(
        spans("See `a` and [x](b.md).\n\nNext `c`\n"),
        [(1, "a".to_owned()), (3, "c".to_owned())]
    );
    assert_eq!(
        targets("See `a` and [x](b.md).\n"),
        [(1, "b.md".to_owned())]
    );
}

#[test]
fn a_fenced_block_hides_its_spans_and_links_and_keeps_its_lines() {
    let text = "```\n`crates/x`\n[a](b.md)\n```\nafter `d`\n";
    assert_eq!(spans(text), [(5, "d".to_owned())]);
    assert_eq!(targets(text), []);
    let fenced: Vec<(u32, String)> = scan(text)
        .fenced
        .into_iter()
        .map(|line| (line.line, line.text))
        .collect();
    assert_eq!(
        fenced,
        [(2, "`crates/x`".to_owned()), (3, "[a](b.md)".to_owned())]
    );
}

#[test]
fn a_tilde_fence_closes_only_with_a_tilde_fence() {
    assert_eq!(spans("~~~\n```\n~~~\n`x`\n"), [(4, "x".to_owned())]);
}

#[test]
fn a_fence_closes_only_with_a_run_at_least_as_long_as_its_opening() {
    assert_eq!(spans("````\n```\n````\n`x`\n"), [(4, "x".to_owned())]);
    assert_eq!(spans("````\n`y`\n`````\n`x`\n"), [(4, "x".to_owned())]);
}

#[test]
fn an_unclosed_fence_runs_to_the_end_of_the_file() {
    assert_eq!(spans("```\n`x`\n"), []);
}

#[test]
fn a_backtick_fence_with_a_backtick_in_its_info_string_is_text() {
    assert_eq!(scan("```a`b\n").fenced, []);
}

#[test]
fn a_span_can_be_opened_by_several_backticks_and_hold_one() {
    assert_eq!(spans("``a ` b``"), [(1, "a ` b".to_owned())]);
    assert_eq!(spans("``crates/x``"), [(1, "crates/x".to_owned())]);
}

#[test]
fn an_unmatched_backtick_is_text() {
    assert_eq!(spans("a ` b"), []);
    assert_eq!(spans("a `` b ` c"), []);
}

#[test]
fn an_escaped_backtick_does_not_open_a_span() {
    assert_eq!(spans("\\`a` b"), []);
}

#[test]
fn a_span_may_run_over_lines_of_its_paragraph() {
    assert_eq!(spans("`a\nb`\n"), [(1, "a b".to_owned())]);
}

#[test]
fn a_span_is_trimmed_and_its_line_breaks_become_spaces() {
    assert_eq!(spans("`  a  `"), [(1, "a".to_owned())]);
}

#[test]
fn a_span_in_an_html_comment_counts() {
    assert_eq!(spans("<!-- `crates/x/` -->"), [(1, "crates/x/".to_owned())]);
}

#[test]
fn escaped_brackets_are_text_not_links() {
    assert_eq!(targets("\\[x](y.md) [z\\](w.md)"), []);
}

#[test]
fn a_bracket_inside_a_span_opens_no_link() {
    assert_eq!(targets("`[`](x.md)"), []);
}

#[test]
fn link_titles_angle_brackets_and_balanced_parentheses_are_read() {
    assert_eq!(
        targets("[a](b.md \"t\") [c](<d e.md>) [f](g(1).md 'x') [h](i.md)"),
        [
            (1, "b.md".to_owned()),
            (1, "d e.md".to_owned()),
            (1, "g(1).md".to_owned()),
            (1, "i.md".to_owned()),
        ]
    );
}

#[test]
fn an_image_is_a_link_and_an_anchor_stays_in_the_target() {
    assert_eq!(
        targets("![alt](img.png) [x](y.md#z)"),
        [(1, "img.png".to_owned()), (1, "y.md#z".to_owned())]
    );
}

#[test]
fn a_link_with_brackets_in_its_text_is_one_link_at_its_opening_bracket() {
    assert_eq!(targets("x\n[a [b] c](d.md)"), [(2, "d.md".to_owned())]);
}

#[test]
fn text_that_is_not_an_inline_link_is_no_link() {
    assert_eq!(targets("[x](y.md"), []);
    assert_eq!(targets("[x] (y.md)"), []);
    assert_eq!(targets("[x](y.md \"unclosed)"), []);
}

#[test]
fn reference_definitions_are_links_except_footnotes_and_indented_lines() {
    let text = "[one]: a.md\n[four]: <c d.md> \"title\"\n[^two]: note\n    [three]: b.md\n";
    assert_eq!(
        targets(text),
        [(1, "a.md".to_owned()), (2, "c d.md".to_owned())]
    );
}

#[test]
fn a_reference_definition_needs_a_label_with_text() {
    assert_eq!(targets("[ ]: a.md\n"), []);
}

#[test]
fn a_definition_needs_a_destination_and_at_most_a_title_after_it() {
    assert_eq!(targets("[Note]: this is a note\n"), []);
    assert_eq!(
        targets("[Note]: a.md \"the title\"\n"),
        [(1, "a.md".to_owned())]
    );
    assert_eq!(targets("[Note]: a.md (title)\n"), [(1, "a.md".to_owned())]);
    assert_eq!(targets("[Note]: a.md 'it'\n"), [(1, "a.md".to_owned())]);
}

#[test]
fn a_definition_does_not_continue_a_paragraph_but_follows_a_heading() {
    assert_eq!(targets("Some text\n[x]: a.md\n"), []);
    assert_eq!(targets("# Title\n[x]: a.md\n"), [(2, "a.md".to_owned())]);
    assert_eq!(
        targets("[x]: a.md\n[y]: b.md\nmore text\n[z]: c.md\n"),
        [(1, "a.md".to_owned()), (2, "b.md".to_owned())]
    );
}

#[test]
fn a_span_on_a_heading_line_is_read_on_its_own() {
    assert_eq!(
        spans("# `crates/x`\n`y`\n"),
        [(1, "crates/x".to_owned()), (2, "y".to_owned())]
    );
    assert_eq!(spans("#hashtag `z`\n"), [(1, "z".to_owned())]);
}
