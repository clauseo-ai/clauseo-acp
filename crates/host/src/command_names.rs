//! The file names a turn's shell commands name, for the after-turn checkpoint to tell a file
//! the agent made through a shell (pandoc's `-o "Reply.docx"`, `> notes.txt`, `cp a b`) from
//! one the lawyer saved meanwhile: a new file is the agent's own only when a tool call or such a
//! command names it (`clauseo_matters::Matter::after_turn_named`).
//!
//! Each text (the command's title, its input and output, its text content) is read as command
//! lines: split on spaces and the shell's operators, quotes kept together, and each word kept as
//! its last path component (after the last `/` or `\`, and after an `=`). A name matches a file
//! only whole: `Reply.docx.bak` names no `Reply.docx`. JSON input or output is read string by
//! string. Kept in memory for the turn only, never logged.

use std::collections::BTreeSet;

/// The most names one turn keeps: a command that lists a large folder adds no more.
const MAX_NAMES: usize = 4096;
/// The longest name kept, in bytes: longer is no file name.
const MAX_NAME: usize = 255;

/// Adds the names in `text`, read as JSON when it is (every string in it), else as command lines.
pub(crate) fn add_json(names: &mut BTreeSet<String>, text: &str) {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => add_value(names, &value),
        Err(_) => add_words(names, text),
    }
}

fn add_value(names: &mut BTreeSet<String>, value: &serde_json::Value) {
    match value {
        serde_json::Value::String(text) => add_words(names, text),
        serde_json::Value::Array(items) => items.iter().for_each(|item| add_value(names, item)),
        serde_json::Value::Object(fields) => fields.values().for_each(|v| add_value(names, v)),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Adds the names in `text`, read as command lines.
pub(crate) fn add_words(names: &mut BTreeSet<String>, text: &str) {
    for word in words(text) {
        let after_equals = word.rsplit_once('=').map(|(_, value)| value);
        for part in std::iter::once(word.as_str()).chain(after_equals) {
            let name = part.rsplit(['/', '\\']).next().unwrap_or(part);
            add(names, name);
            // A name ending a sentence of output: "Wrote Reply.docx."
            let trimmed = name.trim_end_matches([',', ':', ';', '!', '?']);
            add(names, trimmed.strip_suffix('.').unwrap_or(trimmed));
        }
    }
}

fn add(names: &mut BTreeSet<String>, name: &str) {
    if names.len() < MAX_NAMES
        && !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && !names.contains(name)
    {
        names.insert(name.to_owned());
    }
}

/// The words of `text` as a shell splits them: on white space and `< > | ; & ( )` and the
/// backtick, outside quotes; a quote's content is part of its word, and `\"` inside double
/// quotes is a quote.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(open) if c == open => quote = None,
            Some('"') if c == '\\' && chars.peek() == Some(&'"') => {
                word.push('"');
                chars.next();
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() || "<>|;&()`".contains(c) => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            Some(_) | None => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[cfg(test)]
mod tests;
