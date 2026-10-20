//! A small YAML tree with line numbers, built from `saphyr-parser`'s events, and the comments of a
//! file, which every YAML tree drops.
//!
//! An alias is the same `Rc` as the node its anchor names, so a walk that remembers the nodes it
//! has seen visits each once, however often aliases repeat them. Line numbers come from the
//! parser's markers: `line()` counts from 1 and `col()` counts characters from 0 (saphyr-parser
//! 0.2.2, `src/scanner.rs`, `Marker`; checked by the tests in `pins/tests.rs`).

use saphyr_parser::{Event, Marker, Parser, ScalarStyle, Span};
use std::collections::HashMap;
use std::rc::Rc;

/// A node of the tree and the lines it spans.
#[derive(Debug)]
pub(super) struct Node {
    /// The line it starts on, from 1.
    pub line: u32,
    /// The line it ends on: a block scalar spans several.
    pub end_line: u32,
    pub value: Value,
}

/// What a node holds. Keys of a mapping are nodes too, in the order they are written.
#[derive(Debug)]
pub(super) enum Value {
    Scalar(String),
    Seq(Vec<Rc<Node>>),
    Map(Vec<(Rc<Node>, Rc<Node>)>),
}

impl Node {
    /// The text of a scalar, `None` for a sequence or a mapping.
    pub(super) fn scalar(&self) -> Option<&str> {
        match &self.value {
            Value::Scalar(text) => Some(text),
            Value::Seq(_) | Value::Map(_) => None,
        }
    }

    /// The entries of a mapping, with those a merge key (`<<: *anchor`) brings in, in order;
    /// empty for anything else.
    pub(super) fn entries(&self) -> Vec<(Rc<Node>, Rc<Node>)> {
        let mut out = Vec::new();
        self.collect_entries(&mut out, 0);
        out
    }

    fn collect_entries(&self, out: &mut Vec<(Rc<Node>, Rc<Node>)>, depth: usize) {
        // A merge can only name a node that was complete before it, so merges never loop; the
        // limit only bounds a long chain of them.
        if depth > MAX_MERGE_DEPTH {
            return;
        }
        let Value::Map(entries) = &self.value else {
            return;
        };
        for (key, value) in entries {
            if key.scalar() == Some("<<") {
                match &value.value {
                    Value::Seq(items) => {
                        for item in items {
                            item.collect_entries(out, depth.saturating_add(1));
                        }
                    }
                    Value::Map(_) => value.collect_entries(out, depth.saturating_add(1)),
                    Value::Scalar(_) => out.push((Rc::clone(key), Rc::clone(value))),
                }
            } else {
                out.push((Rc::clone(key), Rc::clone(value)));
            }
        }
    }
}

/// How deep merges may nest before `entries` stops following them.
const MAX_MERGE_DEPTH: usize = 64;

/// A parsed file: the root node of each document, and where its scalars are.
pub(super) struct Doc {
    roots: Vec<Rc<Node>>,
    /// The span and style of every scalar, so `comments` can tell a `#` in a string from a
    /// comment.
    scalars: Vec<(Span, ScalarStyle)>,
}

/// Why a file is not YAML, at the line the parser stopped on.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NotYaml {
    pub line: u32,
    /// The parser's description, e.g. `did not find expected node content`.
    pub info: String,
}

/// Parses `text`, every document of it.
pub(super) fn parse(text: &str) -> Result<Doc, NotYaml> {
    let mut builder = Builder::default();
    for event in Parser::new_from_str(text) {
        let (event, span) = event.map_err(|error| NotYaml {
            line: line(*error.marker()),
            info: error.info().to_owned(),
        })?;
        builder.on(event, span);
    }
    Ok(Doc {
        roots: builder.roots,
        scalars: builder.scalars,
    })
}

/// A sequence or mapping whose end has not come yet.
struct Open {
    line: u32,
    anchor: usize,
    is_map: bool,
    items: Vec<Rc<Node>>,
}

#[derive(Default)]
struct Builder {
    open: Vec<Open>,
    anchors: HashMap<usize, Rc<Node>>,
    roots: Vec<Rc<Node>>,
    scalars: Vec<(Span, ScalarStyle)>,
}

impl Builder {
    fn on(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::Scalar(text, style, anchor, _) => {
                self.scalars.push((span, style));
                let node = Node {
                    line: line(span.start),
                    end_line: line(span.end),
                    value: Value::Scalar(text.into_owned()),
                };
                self.finish(node, anchor);
            }
            Event::Alias(anchor) => {
                // The parser refuses an unknown anchor. One whose node is still open (an alias
                // inside its own anchor's node) has no node yet, and stands as an empty scalar.
                let node = self.anchors.get(&anchor).map_or_else(
                    || {
                        Rc::new(Node {
                            line: line(span.start),
                            end_line: line(span.end),
                            value: Value::Scalar(String::new()),
                        })
                    },
                    Rc::clone,
                );
                self.add(node);
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.open.push(Open {
                    line: line(span.start),
                    anchor,
                    is_map: matches!(event, Event::MappingStart(..)),
                    items: Vec::new(),
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some(open) = self.open.pop() {
                    let value = if open.is_map {
                        let mut pairs = Vec::new();
                        let mut items = open.items.into_iter();
                        while let (Some(key), Some(value)) = (items.next(), items.next()) {
                            pairs.push((key, value));
                        }
                        Value::Map(pairs)
                    } else {
                        Value::Seq(open.items)
                    };
                    let node = Node {
                        line: open.line,
                        end_line: line(span.end),
                        value,
                    };
                    self.finish(node, open.anchor);
                }
            }
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd => {}
        }
    }

    /// Records a complete node under its anchor (0 is none) and adds it to its parent.
    fn finish(&mut self, node: Node, anchor: usize) {
        let node = Rc::new(node);
        if anchor != 0 {
            self.anchors.insert(anchor, Rc::clone(&node));
        }
        self.add(node);
    }

    fn add(&mut self, node: Rc<Node>) {
        match self.open.last_mut() {
            Some(open) => open.items.push(node),
            None => self.roots.push(node),
        }
    }
}

impl Doc {
    /// The root node of each document, in order.
    pub(super) fn roots(&self) -> &[Rc<Node>] {
        &self.roots
    }

    /// Every comment, as its line and the text after its `#`. A `#` starts a comment at the
    /// start of a line or after a space or tab, unless a scalar's span holds it (a `#` inside a
    /// quoted string or a block scalar is text).
    pub(super) fn comments<'t>(&self, text: &'t str) -> Vec<(u32, &'t str)> {
        let lines: Vec<&str> = text.lines().collect();
        let scalars: Vec<Place> = self
            .scalars
            .iter()
            .map(|(span, style)| {
                let start = (line(span.start), span.start.col());
                let end = match style {
                    ScalarStyle::DoubleQuoted => quoted_end(&lines, start, '"'),
                    ScalarStyle::SingleQuoted => quoted_end(&lines, start, '\''),
                    _ => (line(span.end), span.end.col()),
                };
                Place { start, end }
            })
            .collect();
        let mut out = Vec::new();
        for (index, source) in lines.iter().enumerate() {
            let at = crate::checks::line_at(index);
            let mut previous = None;
            for (col, (offset, ch)) in source.char_indices().enumerate() {
                let starts = previous.is_none_or(|before: char| before == ' ' || before == '\t');
                if ch == '#' && starts && !scalars.iter().any(|place| place.holds((at, col))) {
                    let rest = source.get(offset.saturating_add(1)..).unwrap_or_default();
                    out.push((at, rest));
                    break;
                }
                previous = Some(ch);
            }
        }
        out
    }
}

/// Where a scalar is: from its first character to just past its last, as (line from 1, column
/// in characters from 0).
struct Place {
    start: (u32, usize),
    end: (u32, usize),
}

impl Place {
    fn holds(&self, here: (u32, usize)) -> bool {
        self.start <= here && here < self.end
    }
}

/// Just past the closing quote of the quoted scalar whose opening quote is at `start`.
/// saphyr-parser 0.2.2 ends a quoted scalar's span after the blanks and comment that follow it,
/// so the end is found here: `\` escapes the next character in double quotes, and `''` is a
/// quote in single quotes. An unclosed one (the parser refuses it) runs to the end.
fn quoted_end(lines: &[&str], start: (u32, usize), quote: char) -> (u32, usize) {
    let (first, open) = start;
    let first_index = usize::try_from(first.saturating_sub(1)).unwrap_or(usize::MAX);
    for (index, source) in lines.iter().enumerate().skip(first_index) {
        let at = crate::checks::line_at(index);
        let skip = if index == first_index {
            open.saturating_add(1)
        } else {
            0
        };
        let mut chars = source.chars().enumerate().skip(skip).peekable();
        while let Some((col, ch)) = chars.next() {
            if quote == '"' && ch == '\\' {
                chars.next();
            } else if ch == quote {
                if quote == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    return (at, col.saturating_add(1));
                }
            }
        }
    }
    (u32::MAX, 0)
}

/// The line of a marker, from 1.
fn line(marker: Marker) -> u32 {
    u32::try_from(marker.line()).unwrap_or(u32::MAX)
}
