//! One paragraph line's content read into a [`BlacklineParagraph`], for `parse.rs`. The
//! grammar is stetdoc's `stet/1` (its design notes, §5.2 and §5.5).
//!
//! It answers:
//! - what a paragraph's text is: `{++…++}{#ids X}` inserted and `{--…--}{#ids X}` deleted
//!   (a change inside a change, `{++a{--b--}{#2 X}c++}{#1 X}`, is deleted when either is), the
//!   rest kept, with the escapes `\{`, `\}`, `\\`, `\t` (a tab), `\n` (a line break) and
//!   `\u{hex}`;
//! - what the tags at the start are, each dropped: the style (`{style "heading 1"}`), a format
//!   change (`{rev run #13 C}`, which makes the paragraph `formatted`), the paragraph-mark
//!   change (`{¶+ #5 C}`, `{¶- #3 C}`), which says the paragraph was inserted or deleted,
//!   a bullet (`{•}`), `{joins-next}`, a table cell's tags
//!   (`{+}` for a later block of the same cell, `{span 2}`, `{vmerge restart}`, `{vmerge}`,
//!   `{cell inserted …}`) and `{hidden-para}` (a paragraph Word hides, shown as any other); a
//!   list number (`{§"1."}`, `{§"1." ghost}` for a deleted one) reads as its text first;
//! - what each inline marker reads as, by its first word: nothing for comment anchors (`{c0}`,
//!   `{/c0}`), notes (`{fn1}`, `{en1}`, `{fn.}`), links (`{link "…"}`, `{/link}`), content
//!   controls (`{ctl …}`, `{/ctl}`), automatic numbers (`{page}`) and objects (`{img "…"}`, a
//!   text box its text); a field (`{field PAGE "2"}`) its result, math its text, a page or column
//!   break a line break, hidden text (`{hidden "…"}`, `{looks-hidden white "…"}`) its text, as
//!   stetdoc's view shows it;
//! - how a paragraph changed: `added` when it has `{¶+ …}` and all its text is inserted,
//!   `removed` when it has `{¶- …}` and all of it is deleted, `formatted` when nothing in it is
//!   inserted or deleted but it has a format change (`{rev …}`, which stetdoc prints for a
//!   tracked bold, strike-through or hide), `same` when it has neither, and `changed`
//!   otherwise; and whether it holds hidden text.
//!
//! A newer stetdoc may add words: words after the ones a known marker needs are ignored, and a
//! marker whose first word is not known is left out and noted ([`Seen::unknown`]). What does
//! not parse (a brace or escape out of place, a change with no id, changes nested past
//! [`MAX_CHANGE_DEPTH`]) is an error naming the rule, never the text.

use clauseo_protocol::{BlacklineParagraph, BlacklineRun, ParagraphChange, RunKind};

/// The deepest a change may sit inside other changes; stetdoc nests one in another (a deletion
/// inside an insertion), and the cap keeps the reader's recursion small on any input.
pub(crate) const MAX_CHANGE_DEPTH: usize = 8;

/// What the paragraphs read so far held that the lawyer should know of.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seen {
    /// Text Word hides or that is hard to see: `{hidden …}`, `{looks-hidden …}`,
    /// `{hidden-para}`.
    pub(crate) hidden: bool,
    /// A marker whose first word this reader does not know, left out.
    pub(crate) unknown: bool,
}

/// One token of a marker's body: a bare word, or a quoted string's text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok<'a> {
    Word(&'a str),
    Quoted(String),
}

/// A marker's body as tokens: words split at single spaces, quoted strings read with their
/// escapes. `name=value` stays one word, and `name="text"` a word then the quoted text.
fn tokens(body: &str) -> Result<Vec<Tok<'_>>, &'static str> {
    let mut toks = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start_matches(' ');
        if rest.is_empty() {
            return Ok(toks);
        }
        if rest.starts_with('"') {
            let (text, after) = quoted(rest)?;
            toks.push(Tok::Quoted(text));
            rest = after;
        } else {
            let end = rest.find([' ', '"']).unwrap_or(rest.len());
            toks.push(Tok::Word(rest.get(..end).unwrap_or_default()));
            rest = rest.get(end..).unwrap_or_default();
        }
    }
}

/// Whether a marker's `body` is a table cell's tag: `+`, `span <n>`, `vmerge`,
/// `vmerge restart`, or a cell's change (`cell inserted #3 C`); words after these are ignored.
pub(super) fn cell_tag(body: &str) -> bool {
    let mut words = body.split(' ');
    match words.next().unwrap_or_default() {
        "+" | "vmerge" => true,
        "span" => words
            .next()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        "cell" => words.next().is_some_and(|change| !change.is_empty()),
        _ => false,
    }
}

/// The paragraph-mark change a paragraph starts with, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Inserted,
    Deleted,
}

/// One paragraph from its content: the tags, then the text; `seen` notes what it holds.
pub(super) fn paragraph(
    content: &str,
    seen: &mut Seen,
) -> Result<BlacklineParagraph, &'static str> {
    // What this paragraph holds, added to `seen` once it reads.
    let mut own = Seen::default();
    let mut formatted = false;
    let mut rest = content;
    let mut mark = Mark::None;
    let mut label: Option<(String, bool)> = None;
    while rest.starts_with('{') && !rest.starts_with("{++") && !rest.starts_with("{--") {
        let (body, after) = marker(rest)?;
        let first = body.split(' ').next().unwrap_or_default();
        if first == "¶+" {
            mark = Mark::Inserted;
        } else if first == "¶-" {
            mark = Mark::Deleted;
        } else if let Some(number) = body.strip_prefix('§') {
            label = Some(list_number(number)?);
        } else if first == "hidden-para" {
            own.hidden = true;
        } else if first == "rev" {
            formatted = true;
        } else if !matches!(first, "style" | "•" | "joins-next") && !cell_tag(body) {
            break;
        }
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    let mut runs: Vec<BlacklineRun> = Vec::new();
    inline(&mut rest, RunKind::Same, None, &mut runs, &mut own)?;
    let all = |kind: RunKind| runs.iter().all(|run| run.kind == kind);
    let change = match mark {
        Mark::Inserted if all(RunKind::Ins) => ParagraphChange::Added,
        Mark::Deleted if all(RunKind::Del) => ParagraphChange::Removed,
        Mark::None if all(RunKind::Same) && formatted => ParagraphChange::Formatted,
        Mark::None if all(RunKind::Same) => ParagraphChange::Same,
        Mark::None | Mark::Inserted | Mark::Deleted => ParagraphChange::Changed,
    };
    if let Some((number, ghost)) = label {
        let kind = match change {
            ParagraphChange::Added if !ghost => RunKind::Ins,
            ParagraphChange::Same
            | ParagraphChange::Changed
            | ParagraphChange::Formatted
            | ParagraphChange::NotShown
            | ParagraphChange::Unknown
                if !ghost =>
            {
                RunKind::Same
            }
            ParagraphChange::Added
            | ParagraphChange::Removed
            | ParagraphChange::Same
            | ParagraphChange::Changed
            | ParagraphChange::Formatted
            | ParagraphChange::NotShown
            | ParagraphChange::Unknown => RunKind::Del,
        };
        let mut numbered = Vec::with_capacity(runs.len() + 1);
        push(&mut numbered, kind, &format!("{number} "));
        for run in runs {
            push(&mut numbered, run.kind, &run.text);
        }
        runs = numbered;
    }
    seen.hidden |= own.hidden;
    seen.unknown |= own.unknown;
    Ok(BlacklineParagraph {
        change,
        runs,
        hidden: own.hidden,
    })
}

/// A list number's text from the marker body after `§`: `"1."`, or `"1." ghost` for one that
/// was deleted (`true`); other words after it are ignored.
fn list_number(body: &str) -> Result<(String, bool), &'static str> {
    let (number, after) = quoted(body)?;
    Ok((number, after.split(' ').any(|word| word == "ghost")))
}

/// Reads text and markers from `rest` into `runs` as `kind` until `end` (`++}` or `--}`, which
/// is consumed) or, with none, the end of the line. `depth` is how many changes it is inside.
fn inline(
    rest: &mut &str,
    kind: RunKind,
    end: Option<(&str, usize)>,
    runs: &mut Vec<BlacklineRun>,
    seen: &mut Seen,
) -> Result<(), &'static str> {
    let mut text = String::new();
    let depth = end.map_or(0, |(_, depth)| depth);
    loop {
        if let Some((end, _)) = end
            && let Some(after) = rest.strip_prefix(end)
        {
            *rest = after;
            break;
        }
        let mut chars = rest.chars();
        let Some(next) = chars.next() else {
            if end.is_some() {
                return Err("an insertion or deletion that does not end");
            }
            break;
        };
        match next {
            '\\' => {
                let (c, after) = escape(rest)?;
                text.push(c);
                *rest = after;
            }
            '{' if rest.starts_with("{++") || rest.starts_with("{--") => {
                if depth >= MAX_CHANGE_DEPTH {
                    return Err("changes nested past the cap");
                }
                push(runs, kind, &text);
                text.clear();
                let (inner, close) = if rest.starts_with("{++") && kind != RunKind::Del {
                    (RunKind::Ins, "++}")
                } else if rest.starts_with("{++") {
                    (RunKind::Del, "++}")
                } else {
                    (RunKind::Del, "--}")
                };
                *rest = rest.get(3..).unwrap_or_default();
                inline(rest, inner, Some((close, depth + 1)), runs, seen)?;
                let (body, after) = marker(rest).map_err(|_no_marker| "a change without its id")?;
                let ids = body.strip_prefix('#').unwrap_or_default();
                if !ids.starts_with(|c: char| c.is_ascii_digit()) {
                    return Err("a change without its id");
                }
                *rest = after;
            }
            '{' => {
                let (body, after) = marker(rest)?;
                text.push_str(&inline_marker(body, seen)?);
                *rest = after;
            }
            '}' => return Err("a closing brace that is not escaped"),
            c => {
                text.push(c);
                *rest = chars.as_str();
            }
        }
    }
    push(runs, kind, &text);
    Ok(())
}

/// What an inline marker reads as, by its first word (the module doc lists them); words after
/// the ones it needs are ignored. A first word this reader does not know reads as nothing and
/// is noted in `seen`.
fn inline_marker(body: &str, seen: &mut Seen) -> Result<String, &'static str> {
    let toks = tokens(body)?;
    let Some(Tok::Word(first)) = toks.first() else {
        return Err("a marker that does not start with a word");
    };
    let first = *first;
    // The first quoted string after `skip` tokens, if any.
    let quoted_after = |skip: usize| {
        toks.iter().skip(skip).find_map(|tok| match tok {
            Tok::Quoted(text) => Some(text.clone()),
            Tok::Word(_) => None,
        })
    };
    let numbered = |prefix: &str| {
        first
            .strip_prefix(prefix)
            .is_some_and(|n| n == "." || (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
    };
    let text = match first {
        "/link" | "/ctl" | "link" | "ctl" | "page" | "date" | "value" => String::new(),
        "img" | "shape" | "chart" | "object" | "textbox" => {
            let boxed = toks.iter().position(|tok| *tok == Tok::Word("box"));
            boxed
                .and_then(|at| quoted_after(at + 1))
                .unwrap_or_default()
        }
        "pagebreak" | "colbreak" => "\n".to_owned(),
        "field" => {
            // `{field KW "result"}`, `{field KW start "…"}`: the keyword may be quoted too.
            let skip = if matches!(toks.get(1), Some(Tok::Quoted(_))) {
                2
            } else {
                1
            };
            quoted_after(skip).ok_or("a field with no result")?
        }
        "math" => quoted_after(1).ok_or("a math marker with no text")?,
        "hidden" | "looks-hidden" => {
            seen.hidden = true;
            quoted_after(1).ok_or("a hidden-text marker with no text")?
        }
        _ if first.starts_with("++") || first.starts_with("--") => {
            return Err("a change where a marker should be");
        }
        _ if numbered("c") || numbered("/c") || numbered("fn") || numbered("en") => String::new(),
        _ => {
            seen.unknown = true;
            String::new()
        }
    };
    Ok(text)
}

/// The body of the marker `rest` starts with (between its braces) and what follows it. Quoted
/// strings inside may hold braces; a brace outside one is an error.
pub(super) fn marker(rest: &str) -> Result<(&str, &str), &'static str> {
    let inner = rest
        .strip_prefix('{')
        .ok_or("markup that does not start with a brace")?;
    let mut quoted = false;
    let mut escaped = false;
    for (at, c) in inner.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '}' if !quoted => {
                let body = inner.get(..at).unwrap_or_default();
                return Ok((body, inner.get(at + 1..).unwrap_or_default()));
            }
            '{' | '\\' if !quoted => return Err("markup inside markup"),
            _ => {}
        }
    }
    Err("markup that does not end")
}

/// The text of the quoted string `body` starts with, its escapes read, and what follows it.
fn quoted(body: &str) -> Result<(String, &str), &'static str> {
    let mut rest = body
        .strip_prefix('"')
        .ok_or("a marker without its quoted text")?;
    let mut text = String::new();
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            None => return Err("quoted text that does not end"),
            Some('"') => return Ok((text, chars.as_str())),
            Some('\\') if rest.starts_with("\\\"") => {
                text.push('"');
                rest = rest.get(2..).unwrap_or_default();
            }
            Some('\\') => {
                let (c, after) = escape(rest)?;
                text.push(c);
                rest = after;
            }
            Some(c) => {
                text.push(c);
                rest = chars.as_str();
            }
        }
    }
}

/// The character the escape `rest` starts with stands for, and what follows it.
fn escape(rest: &str) -> Result<(char, &str), &'static str> {
    let after = rest.get(1..).unwrap_or_default();
    let mut chars = after.chars();
    let c = match chars.next() {
        Some(c @ ('{' | '}' | '\\')) => c,
        Some('t') => '\t',
        Some('n') => '\n',
        Some('u') => {
            let hex = chars
                .as_str()
                .strip_prefix('{')
                .ok_or("an escape the reader does not know")?;
            let end = hex.find('}').ok_or("an escape the reader does not know")?;
            let code = u32::from_str_radix(hex.get(..end).unwrap_or_default(), 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or("an escape the reader does not know")?;
            return Ok((code, hex.get(end + 1..).unwrap_or_default()));
        }
        None | Some(_) => return Err("an escape the reader does not know"),
    };
    Ok((c, chars.as_str()))
}

/// Adds `text` as `kind` to `runs`: joined to the last run when it has the same kind, and
/// nothing when it is empty.
fn push(runs: &mut Vec<BlacklineRun>, kind: RunKind, text: &str) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => runs.push(BlacklineRun {
            kind,
            text: text.to_owned(),
        }),
    }
}
