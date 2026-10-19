//! Reads the Markdown the docs check needs with `pulldown-cmark`: inline code spans, links (inline
//! links and images, reference definitions, and the `href` and `src` of raw HTML `<a>` and
//! `<img>` tags) and the lines of code blocks. The body of an HTML comment is read as Markdown
//! too: agents read the raw files, so a path or command in a comment is read like any other.

mod html;

use crate::checks;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

/// What `scan` found in one Markdown file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Scan {
    /// Inline code spans outside code blocks, comments included.
    pub spans: Vec<Span>,
    /// Inline links and images, HTML link attributes and reference definitions.
    pub links: Vec<Link>,
    /// The lines of fenced and indented code blocks, in file order, fence lines left out.
    pub code: Vec<Line>,
}

/// An inline code span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Span {
    /// The 1-based line of the opening backticks.
    pub line: u32,
    /// The content, with each line break as a space and the ends trimmed.
    pub text: String,
}

/// A link destination, backslash escapes and entities read, anchor and query kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    /// The 1-based line of the link's `[` or `!`, of the definition's label, or of the HTML tag.
    pub line: u32,
    pub target: String,
}

/// One line of a code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Line {
    /// The 1-based line in the file.
    pub line: u32,
    /// The line's code, container markers and the block's indent removed.
    pub text: String,
}

/// Reads `text`, the contents of one Markdown file.
pub(super) fn scan(text: &str) -> Scan {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let line_of = |offset: usize| {
        checks::line_at(
            starts
                .partition_point(|start| *start <= offset)
                .saturating_sub(1),
        )
    };
    let mut scan = Scan::default();
    read(text, &line_of, &mut scan);
    scan
}

/// The extensions GitHub renders: tables, footnotes, strikethrough, task lists and its alerts.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// Reads `text` into `scan`. `line_of` gives the 1-based file line of a byte offset in `text`.
fn read(text: &str, line_of: &dyn Fn(usize) -> u32, scan: &mut Scan) {
    let events = Parser::new_ext(text, options()).into_offset_iter();
    let mut definitions: Vec<(usize, String)> = events
        .reference_definitions()
        .iter()
        .map(|(_, definition)| (definition.span.start, definition.dest.to_string()))
        .collect();
    definitions.sort();
    scan.links
        .extend(definitions.into_iter().map(|(start, target)| Link {
            line: line_of(start),
            target,
        }));
    let mut code: Option<Pieces> = None;
    let mut html: Option<Pieces> = None;
    for (event, range) in events {
        match event {
            Event::Code(span) => scan.spans.push(Span {
                line: line_of(range.start),
                text: span.trim().to_owned(),
            }),
            // A reference link's destination is its definition's, which is read at its own line.
            Event::Start(
                Tag::Link {
                    link_type: LinkType::Inline,
                    dest_url,
                    ..
                }
                | Tag::Image {
                    link_type: LinkType::Inline,
                    dest_url,
                    ..
                },
            ) => scan.links.push(Link {
                line: line_of(range.start),
                target: dest_url.to_string(),
            }),
            Event::Start(Tag::CodeBlock(_)) => code = Some(Pieces::default()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(block) = code.take() {
                    block.code_lines(line_of, &mut scan.code);
                }
            }
            Event::Text(piece) => {
                if let Some(block) = code.as_mut() {
                    block.push(&piece, range.start);
                }
            }
            Event::Start(Tag::HtmlBlock) => html = Some(Pieces::default()),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(block) = html.take() {
                    read_html(&block.text, &|offset| block.line(offset, line_of), scan);
                }
            }
            Event::Html(piece) => match html.as_mut() {
                Some(block) => block.push(&piece, range.start),
                None => read_html(&piece, &|offset| line_of(range.start + offset), scan),
            },
            Event::InlineHtml(piece) => {
                read_html(&piece, &|offset| line_of(range.start + offset), scan);
            }
            _ => {}
        }
    }
}

/// Reads raw HTML: the `<a href>` and `<img src>` tags outside comments, and each comment's body
/// as Markdown. An unclosed comment runs to the end of `html`, as it does in an HTML block.
fn read_html(html: &str, line_of: &dyn Fn(usize) -> u32, scan: &mut Scan) {
    let mut from = 0;
    while let Some(open) = html.get(from..).and_then(|rest| rest.find("<!--")) {
        let open = from + open;
        let before = html.get(from..open).unwrap_or_default();
        html::read_tags(before, &|offset| line_of(from + offset), scan);
        let body_start = open + "<!--".len();
        let (body_end, next) = html
            .get(body_start..)
            .and_then(|rest| rest.find("-->"))
            .map_or((html.len(), html.len()), |close| {
                (body_start + close, body_start + close + "-->".len())
            });
        let body = html.get(body_start..body_end).unwrap_or_default();
        read(body, &|offset| line_of(body_start + offset), scan);
        from = next;
    }
    let rest = html.get(from..).unwrap_or_default();
    html::read_tags(rest, &|offset| line_of(from + offset), scan);
}

/// The text of one code or HTML block, joined from the events that carry it, with where each
/// piece starts in the file. Container markers (`>`, a list item's indent) are not in it.
#[derive(Debug, Default)]
struct Pieces {
    text: String,
    /// Each piece's start in `text` and in the parsed source.
    starts: Vec<(usize, usize)>,
}

impl Pieces {
    /// Adds `piece`, which starts at byte `source` of the parsed text.
    fn push(&mut self, piece: &str, source: usize) {
        self.starts.push((self.text.len(), source));
        self.text.push_str(piece);
    }

    /// The file line of byte `offset` of `text`: the line its piece starts on, plus the line
    /// breaks before it in that piece.
    fn line(&self, offset: usize, line_of: &dyn Fn(usize) -> u32) -> u32 {
        let index = self
            .starts
            .partition_point(|(start, _)| *start <= offset)
            .saturating_sub(1);
        let Some(&(start, source)) = self.starts.get(index) else {
            return line_of(0);
        };
        let breaks = self
            .text
            .get(start..offset)
            .map_or(0, |before| before.matches('\n').count());
        line_of(source).saturating_add(u32::try_from(breaks).unwrap_or(u32::MAX))
    }

    /// Adds each line of the block to `code`, at its file line.
    fn code_lines(&self, line_of: &dyn Fn(usize) -> u32, code: &mut Vec<Line>) {
        let mut offset = 0;
        for raw in self.text.split_inclusive('\n') {
            code.push(Line {
                line: self.line(offset, line_of),
                text: raw.trim_end_matches(['\n', '\r']).to_owned(),
            });
            offset += raw.len();
        }
    }
}

#[cfg(test)]
mod tests;
