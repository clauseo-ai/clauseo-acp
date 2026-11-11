//! Reads what `stetdoc view --all --json` prints for a redline into a [`WordBlackline`]: pure,
//! no I/O. The format was taken from stetdoc 0.9.0's output (the release pin is v0.9.3), from
//! real samples (`tests/fixtures/blackline/`), not from a written grammar, so anything the
//! samples did not show fails the read: a blackline is never a guess.
//!
//! It answers:
//! - which lines are blocks: `<ref>` or `<ref> <content>`, where a `p` ref is a paragraph (a
//!   table cell's included), a `t` ref opens a table (`{table cols=2}`) and an `r` ref a row
//!   (`{row}`, `{row inserted #16 C}`); a line that starts with `{` is the view's own
//!   (`{stet ...}`, `{summary: ...}`, `{/table}`, `{section 1 ends: ...}`, `{position: ...}`),
//!   and everything after `{appendix}` (the notes and comments the page cites) is skipped;
//! - what a paragraph's text is: `{++…++}{#ids X}` inserted and `{--…--}{#ids X}` deleted, the
//!   rest kept, with the escapes `\{`, `\}`, `\\`, `\t` (a tab), `\n` (a line break) and
//!   `\u{hex}`;
//! - what markup is dropped: at the start, the style (`{style "heading 1"}`), a format change
//!   (`{rev run #13 C}`) and the paragraph-mark change (`{¶+ #5 C}`, `{¶- #3 C}`), which says the
//!   paragraph was inserted or deleted; inline, comment anchors (`{c0}`, `{/c0}`), a footnote's
//!   reference (`{fn1}`), links (`{link "…"}`, `{/link}`), content controls (`{ctl …}`,
//!   `{/ctl}`) and images (`{img "…"}`). A field (`{field PAGE "2"}`) reads as its result, a
//!   page break (`{pagebreak}`) as a line break, and a list number (`{§"1."}`, `{§"1." ghost}`
//!   for a deleted one) as its text before the paragraph's;
//! - how a paragraph changed: `added` when it has `{¶+ …}` and all its text is inserted,
//!   `removed` when it has `{¶- …}` and all of it is deleted, `same` when nothing in it is
//!   inserted or deleted, and `changed` otherwise.
//!
//! Any other `{…}` not escaped, any other escape, any other ref, and a view that is not a
//! redline make the read fail with [`Unreadable`], which names the line and why, never the
//! document's text. A view with any `warnings` (such as `read_anomaly`, content that exists
//! but is not shown) fails too, by its warning codes alone: a part never passes for the whole.

use std::fmt;

use clauseo_protocol::{BlacklineParagraph, BlacklineRun, ParagraphChange, RunKind, WordBlackline};
use serde::Deserialize;
use serde::de::IgnoredAny;

/// Why the view could not be read: the line of its text (0 for the JSON around it) and the
/// rule it broke. It holds no document text, so it can go in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable {
    /// The line of the view's text, from 1; 0 for the JSON around it.
    pub(crate) line: usize,
    /// The rule the line broke.
    pub(crate) why: &'static str,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.why)
    }
}

/// Why a view gives no blackline: it could not be read, or stetdoc warned about it (content
/// not shown, say) and a part must never pass for the whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ViewFailure {
    /// The view broke a rule of the format.
    Unreadable(Unreadable),
    /// stetdoc's `warnings`, by code alone (such as `read_anomaly`): never their messages,
    /// which may quote the document.
    Warned(Vec<String>),
}

impl fmt::Display for ViewFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(unreadable) => {
                write!(f, "its output could not be read at {unreadable}")
            }
            Self::Warned(codes) => write!(f, "it warned: {}", codes.join(", ")),
        }
    }
}

/// The fields of the view's JSON the read needs; the others are ignored.
#[derive(Deserialize)]
struct View {
    mode: String,
    text: String,
    #[serde(default)]
    next: Option<IgnoredAny>,
    position: Position,
    #[serde(default)]
    warnings: Vec<Warning>,
}

/// One of stetdoc's warnings; only its code is read.
#[derive(Deserialize)]
struct Warning {
    #[serde(default)]
    code: String,
}

/// How much of the document the page shows, in blocks.
#[derive(Deserialize)]
struct Position {
    shown: u64,
    total: u64,
}

/// The blackline in `json`, the stdout of `stetdoc view --all --json` on a redline.
/// `truncated` is stetdoc's own word: a page that shows fewer blocks than there are, or names a
/// next page. Any warning in the view (such as `read_anomaly`: content exists that is not
/// shown) fails it as [`ViewFailure::Warned`]: no warning is known to be harmless.
pub(crate) fn read_view(json: &str) -> Result<WordBlackline, ViewFailure> {
    let json_error = |why| ViewFailure::Unreadable(Unreadable { line: 0, why });
    let view: View =
        serde_json::from_str(json).map_err(|_bad| json_error("the JSON is not a view"))?;
    if view.mode != "redline" {
        return Err(json_error("the view is not a redline"));
    }
    if !view.warnings.is_empty() {
        let codes = view
            .warnings
            .iter()
            .map(|warning| code_word(&warning.code))
            .collect();
        return Err(ViewFailure::Warned(codes));
    }
    let truncated = view.position.shown < view.position.total || view.next.is_some();
    Ok(WordBlackline {
        paragraphs: read_text(&view.text).map_err(ViewFailure::Unreadable)?,
        truncated,
    })
}

/// A warning's code as the log may hold it: a short word of stetdoc's, or `other` for anything
/// else, so no document text reaches the log.
fn code_word(code: &str) -> String {
    let word = code.len() <= 64
        && !code.is_empty()
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit());
    if word {
        code.to_owned()
    } else {
        "other".to_owned()
    }
}

/// The paragraphs of a redline view's text, in order.
pub(crate) fn read_text(text: &str) -> Result<Vec<BlacklineParagraph>, Unreadable> {
    let mut paragraphs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let at = |why| Unreadable {
            line: index + 1,
            why,
        };
        if line.is_empty() {
            continue;
        }
        if line == "{appendix}" {
            break;
        }
        if line.starts_with('{') {
            continue;
        }
        let (letters, content) = block_ref(line).ok_or_else(|| at("a line with no block ref"))?;
        match letters {
            "p" => paragraphs.push(paragraph(content).map_err(at)?),
            "t" if content.starts_with("{table") => {}
            "r" if content.starts_with("{row") => {}
            "t" | "r" => return Err(at("a table or row line without its marker")),
            _ => return Err(at("a block of a kind the reader does not know")),
        }
    }
    Ok(paragraphs)
}

/// A block line's ref letters and its content: `p12 text` is `("p", "text")`, and a bare
/// `p12` has empty content. `None` when the line does not start with a ref.
fn block_ref(line: &str) -> Option<(&str, &str)> {
    let letters = line.find(|c: char| !c.is_ascii_lowercase())?;
    let after = line.get(letters..)?;
    let digits = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    if letters == 0 || digits == 0 {
        return None;
    }
    let content = match after.get(digits..)? {
        "" => "",
        rest => rest.strip_prefix(' ')?,
    };
    Some((line.get(..letters)?, content))
}

/// The paragraph-mark change a paragraph starts with, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Inserted,
    Deleted,
}

/// One paragraph from its content: the prefixes, then the text.
fn paragraph(content: &str) -> Result<BlacklineParagraph, &'static str> {
    let mut rest = content;
    let mut mark = Mark::None;
    let mut label: Option<(String, bool)> = None;
    while rest.starts_with('{') && !rest.starts_with("{++") && !rest.starts_with("{--") {
        let (body, after) = marker(rest)?;
        if body.starts_with("¶+ #") {
            mark = Mark::Inserted;
        } else if body.starts_with("¶- #") {
            mark = Mark::Deleted;
        } else if let Some(number) = body.strip_prefix('§') {
            label = Some(list_number(number)?);
        } else if !body.starts_with("style ") && !body.starts_with("rev ") {
            break;
        }
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    let mut runs: Vec<BlacklineRun> = Vec::new();
    inline(&mut rest, RunKind::Same, None, &mut runs)?;
    let all = |kind: RunKind| runs.iter().all(|run| run.kind == kind);
    let change = match mark {
        Mark::Inserted if all(RunKind::Ins) => ParagraphChange::Added,
        Mark::Deleted if all(RunKind::Del) => ParagraphChange::Removed,
        Mark::None if all(RunKind::Same) => ParagraphChange::Same,
        Mark::None | Mark::Inserted | Mark::Deleted => ParagraphChange::Changed,
    };
    if let Some((number, ghost)) = label {
        let kind = match change {
            ParagraphChange::Added if !ghost => RunKind::Ins,
            ParagraphChange::Same | ParagraphChange::Changed | ParagraphChange::Unknown
                if !ghost =>
            {
                RunKind::Same
            }
            ParagraphChange::Added
            | ParagraphChange::Removed
            | ParagraphChange::Same
            | ParagraphChange::Changed
            | ParagraphChange::Unknown => RunKind::Del,
        };
        let mut numbered = Vec::with_capacity(runs.len() + 1);
        push(&mut numbered, kind, &format!("{number} "));
        for run in runs {
            push(&mut numbered, run.kind, &run.text);
        }
        runs = numbered;
    }
    Ok(BlacklineParagraph { change, runs })
}

/// A list number's text from the marker body after `§`: `"1."`, or `"1." ghost` for one that
/// was deleted (`true`).
fn list_number(body: &str) -> Result<(String, bool), &'static str> {
    let (number, after) = quoted(body)?;
    match after {
        "" => Ok((number, false)),
        " ghost" => Ok((number, true)),
        _ => Err("a list number the reader does not know"),
    }
}

/// Reads text and markers from `rest` into `runs` as `kind` until `end` (`++}` or `--}`, which
/// is consumed) or, with none, the end of the line.
fn inline(
    rest: &mut &str,
    kind: RunKind,
    end: Option<&str>,
    runs: &mut Vec<BlacklineRun>,
) -> Result<(), &'static str> {
    let mut text = String::new();
    loop {
        if let Some(end) = end
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
            '{' if end.is_none() && (rest.starts_with("{++") || rest.starts_with("{--")) => {
                push(runs, kind, &text);
                text.clear();
                let (inner, close) = if rest.starts_with("{++") {
                    (RunKind::Ins, "++}")
                } else {
                    (RunKind::Del, "--}")
                };
                *rest = rest.get(3..).unwrap_or_default();
                inline(rest, inner, Some(close), runs)?;
                let (body, after) = marker(rest).map_err(|_no_marker| "a change without its id")?;
                let ids = body.strip_prefix('#').unwrap_or_default();
                if !ids.starts_with(|c: char| c.is_ascii_digit()) {
                    return Err("a change without its id");
                }
                *rest = after;
            }
            '{' => {
                let (body, after) = marker(rest)?;
                text.push_str(&inline_marker(body)?);
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

/// What an inline marker reads as: nothing for markup that holds no text, the result of a
/// field, a line break for a page break. Any other marker is an error.
fn inline_marker(body: &str) -> Result<String, &'static str> {
    let numbered = |prefix: &str| {
        body.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    if numbered("c") || numbered("/c") || numbered("fn") {
        return Ok(String::new());
    }
    match body {
        "/link" | "/ctl" => return Ok(String::new()),
        "pagebreak" => return Ok("\n".to_owned()),
        _ => {}
    }
    if let Some(url) = body
        .strip_prefix("link ")
        .or_else(|| body.strip_prefix("img "))
    {
        let (_, after) = quoted(url)?;
        return if after.is_empty() {
            Ok(String::new())
        } else {
            Err("a link or image marker the reader does not know")
        };
    }
    if body.starts_with("ctl ") {
        return Ok(String::new());
    }
    if let Some(field) = body.strip_prefix("field ") {
        let start = field.find('"').ok_or("a field with no result")?;
        let (result, after) = quoted(field.get(start..).unwrap_or_default())?;
        return if after.is_empty() {
            Ok(result)
        } else {
            Err("a field marker the reader does not know")
        };
    }
    Err("markup the reader does not know")
}

/// The body of the marker `rest` starts with (between its braces) and what follows it. Quoted
/// strings inside may hold braces; a brace outside one is an error.
fn marker(rest: &str) -> Result<(&str, &str), &'static str> {
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

#[cfg(test)]
mod tests;
