//! An entry's text as Find shows it: plain words on one line. A prompt or an answer is Markdown,
//! so its marks (`**`, `#`, `>`, list bullets, table pipes, code fences) and its links' and
//! images' addresses go, keeping what a reader sees: a link's words, an image's description,
//! code's text. Every line break and every block's end becomes a space, so two lines never glue
//! into one word ("Arbitration" and "and" stay apart), and runs of white space close to one.

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

/// `markdown` as plain words on one line, trimmed.
pub(crate) fn plain(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut out = String::with_capacity(markdown.len());
    // Inside an autolink (`<https://…>`), whose only words are its address.
    let mut autolink = false;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Text(text) | Event::Code(text) => {
                if !autolink {
                    out.push_str(&text);
                }
            }
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink,
                ..
            }) => autolink = true,
            Event::End(TagEnd::Link) => autolink = false,
            // Emphasis, a link or an image, inside a word: nothing between its letters;
            // and what has no words to show.
            Event::Start(
                Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Superscript
                | Tag::Subscript
                | Tag::Link { .. }
                | Tag::Image { .. },
            )
            | Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Image,
            )
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
            // A line's or a block's end, a rule, an inline tag such as `<br>`: a space.
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::End(_)
            | Event::Start(_) => out.push(' '),
        }
    }
    one_line(&out)
}

/// `text` with every run of white space (line breaks included) closed to one space, trimmed.
pub(crate) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{one_line, plain};

    #[test]
    fn marks_and_addresses_go_and_the_words_stay() {
        let text = "## Findings\n\n**Arbitration**\nand the [invoice list](https://example.com/a/b?c=d) \
                    with `clause 4`.\n\n- listed\n- invoices\n\n> quoted\n\n![a scan](scan.png)";
        assert_eq!(
            plain(text),
            "Findings Arbitration and the invoice list with clause 4. listed invoices quoted a scan"
        );
    }

    #[test]
    fn emphasis_inside_a_word_keeps_it_whole() {
        assert_eq!(
            plain("**Arb**itration and ~~old~~new"),
            "Arbitration and oldnew"
        );
    }

    #[test]
    fn a_line_break_is_a_space() {
        assert_eq!(plain("Arbitration\nand"), "Arbitration and");
        assert_eq!(plain("listed  \ninvoices"), "listed invoices");
        assert_eq!(one_line("a\r\nb\u{2028}c\t d"), "a b c d");
    }

    #[test]
    fn an_autolink_and_a_table_keep_no_address_and_no_pipes() {
        assert_eq!(plain("See <https://example.com/x> now"), "See now");
        assert_eq!(
            plain("| Party | Role |\n|---|---|\n| A | Buyer |"),
            "Party Role A Buyer"
        );
    }

    #[test]
    fn devanagari_stays_as_it_is() {
        assert_eq!(plain("**मध्यस्थता**\nऔर"), "मध्यस्थता और");
    }
}
