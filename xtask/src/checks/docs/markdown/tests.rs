//! Tests of what the walk over `pulldown-cmark`'s events keeps, and the lines it gives each item.
//! How Markdown parses is the parser's business; these pin the walk.

use super::*;

/// The spans of `text` as `line: text`.
fn spans(text: &str) -> Vec<String> {
    scan(text)
        .spans
        .into_iter()
        .map(|span| format!("{}: {}", span.line, span.text))
        .collect()
}

/// The links of `text` as `line: target`.
fn links(text: &str) -> Vec<String> {
    scan(text)
        .links
        .into_iter()
        .map(|link| format!("{}: {}", link.line, link.target))
        .collect()
}

/// The code lines of `text` as `line: text`.
fn code(text: &str) -> Vec<String> {
    scan(text)
        .code
        .into_iter()
        .map(|line| format!("{}: {}", line.line, line.text))
        .collect()
}

#[test]
fn a_span_is_at_the_line_of_its_backticks_with_its_line_break_a_space() {
    assert_eq!(spans("# T\n\nsee `a\nb` and `c`\n"), ["3: a b", "4: c"]);
}

#[test]
fn inline_links_images_and_definitions_are_read_and_reference_uses_are_not() {
    let text = "[a](x.md) ![i](y.png)\n\n[use][d] [d]\n\n[d]: z.md\n";
    assert_eq!(links(text), ["5: z.md", "1: x.md", "1: y.png"]);
}

#[test]
fn a_definition_is_at_its_label_s_line_and_an_unused_one_is_read() {
    assert_eq!(links("Text.\n\n[g]:\n  nope.md\n"), ["3: nope.md"]);
}

#[test]
fn code_lines_keep_their_file_lines_in_fences_quotes_and_items() {
    assert_eq!(code("```\na\n\nb\n```\n"), ["2: a", "3: ", "4: b"]);
    assert_eq!(
        code("- item\n\n  > ```\n  > a\n  > b\n  > ```\n"),
        ["4: a", "5: b"]
    );
    assert_eq!(code("Text.\n\n\ta\n\tb\n"), ["3: a", "4: b"]);
}

#[test]
fn html_tags_and_comments_are_read_at_their_lines() {
    let text = "<div>\n<a href=\"x.md\">x</a>\n<!-- `p`\n`q` [l](c.md) -->\n</div>\n";
    let found = scan(text);
    let spans: Vec<(u32, &str)> = found
        .spans
        .iter()
        .map(|s| (s.line, s.text.as_str()))
        .collect();
    assert_eq!(spans, [(3, "p"), (4, "q")]);
    let links: Vec<(u32, &str)> = found
        .links
        .iter()
        .map(|l| (l.line, l.target.as_str()))
        .collect();
    assert_eq!(links, [(2, "x.md"), (4, "c.md")]);
}

#[test]
fn an_inline_tag_is_read_and_text_in_code_is_not() {
    assert_eq!(links("See <img src='a.png'> here.\n"), ["1: a.png"]);
    assert_eq!(
        links("```\n<a href=\"x.md\">\n[l](y.md)\n```\n"),
        Vec::<String>::new()
    );
    assert_eq!(spans("```\n`a`\n```\n"), Vec::<String>::new());
}

/// The spans of `text` that the planned marker follows.
fn planned(text: &str) -> Vec<String> {
    scan(text)
        .spans
        .into_iter()
        .filter(|span| span.planned)
        .map(|span| span.text)
        .collect()
}

#[test]
fn the_planned_marker_is_the_text_right_after_a_span() {
    assert_eq!(
        planned("`a` (planned), `b` (planned) and `c`.\n"),
        ["a", "b"]
    );
    assert_eq!(planned("`a`\n(planned) here\n"), ["a"]);
    assert_eq!(planned("`a` is (planned)\n"), Vec::<String>::new());
    assert_eq!(planned("`a` *(planned)*\n"), Vec::<String>::new());
    assert_eq!(planned("`a`\n\n(planned)\n"), Vec::<String>::new());
}
