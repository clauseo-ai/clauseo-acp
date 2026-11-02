//! The name backstop (clauseo-acp-private#162, review rounds 2 and 3): a shell command whose text
//! or words name a file of the matter that is held open right now is refused, whatever folder
//! games it plays, unless the whole command only reads ([`read_only`]); and `stetdoc`'s writing
//! and serving forms ([`stetdoc()`]).

mod allow;
mod stetdoc;

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;
use unicode_normalization::UnicodeNormalization;

use super::shell::script;
use super::words::{Escape, Segments, ansi_c_char, shell_segments};
use super::{MAX_WORDS, Refusal};

pub(super) use allow::read_only;
pub(super) use stetdoc::{Stetdoc, stetdoc};

/// The most folder entries [`held_files`] reads; a larger matter is read in part, shallow folders
/// first.
const MAX_ENTRIES: usize = 20_000;

/// The most words [`read_only`] and [`named_in`] read; a longer command is not read-only.
const MAX_READ_WORDS: usize = 4096;

/// A file of the matter that is held open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Held {
    pub(super) path: MatterPath,
    /// Its last part, folded as [`fold`] does.
    name: String,
}

impl Held {
    /// The held file at `path`.
    pub(super) fn new(path: MatterPath) -> Self {
        let name = fold(path.as_str().rsplit('/').next().unwrap_or_default());
        Self { path, name }
    }
}

/// The files of the matter `folder` held open by an Office owner file
/// ([`clauseo_matters::held_open`], in each folder that has an owner file `~$…`), read breadth
/// first: no folder whose name starts with `.`, no symlink followed, at most [`MAX_ENTRIES`]
/// entries. A file held on Windows by a share mode alone (no owner file) is not found here.
pub(super) fn held_files(folder: &Path) -> Vec<Held> {
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut held = Vec::new();
    let mut queue: std::collections::VecDeque<PathBuf> = std::iter::once(PathBuf::new()).collect();
    let mut entries = 0;
    while let Some(relative) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(root.join(&relative)) else {
            continue;
        };
        let mut files = Vec::new();
        let mut owned = false;
        for entry in read.flatten() {
            entries += 1;
            if entries > MAX_ENTRIES {
                return held;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') {
                    queue.push_back(relative.join(&name));
                }
            } else if name.starts_with("~$") {
                owned = true;
            } else {
                files.push(name);
            }
        }
        if !owned {
            continue;
        }
        for name in files {
            if let Some(path) = MatterPath::from_relative(&relative.join(&name))
                && clauseo_matters::held_open(&root, &path).is_some()
            {
                held.push(Held::new(path));
            }
        }
    }
    held
}

/// `text` as the file system compares names: lower case on macOS and Windows, and on macOS also
/// composed (NFC), since APFS finds a name typed in either Unicode form; as it is elsewhere.
fn fold(text: &str) -> String {
    if cfg!(target_os = "macos") {
        text.to_lowercase().nfc().collect()
    } else if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// Whether `c` may stand before or after a whole file name in a command: whitespace, a quote or
/// a shell metacharacter; before it also a folder separator.
fn bounds(c: char, before: bool) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\'' | '"'
                | '`'
                | ';'
                | '&'
                | '|'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '='
                | ','
                | ':'
                | '$'
                | '*'
                | '?'
                | '!'
                | '#'
        )
        || (before && matches!(c, '/' | '\\'))
}

/// Whether the folded `text` holds the folded `name` as a whole name ([`bounds`]).
fn holds(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    text.match_indices(name).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at + name.len()..)
            .and_then(|tail| tail.chars().next());
        before.is_none_or(|c| bounds(c, true)) && after.is_none_or(|c| bounds(c, false))
    })
}

/// The first of `held` whose name `text` holds as a whole name.
fn named<'a>(text: &str, held: &'a [Held]) -> Option<&'a Held> {
    let text = fold(text);
    held.iter().find(|file| holds(&text, &file.name))
}

/// The first of `held` that the shell command `command` names: in its text with escaped newlines
/// joined and, under [`Escape::Backslash`], `$'…'` decoded (comments are not dropped); or in one
/// of its first [`MAX_READ_WORDS`] words as the shell reads them, quotes and escapes off
/// (`Con""tract.docx`, `Con\tract.docx`, `My\ Deal.docx`), and the words of each script it
/// hands to `sh -c`, `eval` and the like (one level).
pub(super) fn named_in<'a>(command: &str, escape: Escape, held: &'a [Held]) -> Option<&'a Held> {
    if held.is_empty() {
        return None;
    }
    named(&raw_text(command, escape), held).or_else(|| named_in_words(command, escape, held, 1))
}

/// The first of `held` one of `command`'s words names, and those of its scripts to `depth`.
fn named_in_words<'a>(
    command: &str,
    escape: Escape,
    held: &'a [Held],
    depth: usize,
) -> Option<&'a Held> {
    let mut segments = Segments::new(MAX_READ_WORDS);
    shell_segments(command, escape, &mut segments);
    segments.iter().find_map(|segment| {
        segment
            .words
            .iter()
            .find_map(|word| named(word, held))
            .or_else(|| {
                let (inner, inner_escape) =
                    script(&segment.words, escape).filter(|_| segment.command && depth > 0)?;
                named_in_words(&inner, inner_escape, held, depth - 1)
            })
    })
}

/// `command` with escaped newlines joined and (POSIX) `$'…'` replaced by its decoded text.
fn raw_text(command: &str, escape: Escape) -> String {
    let mut text = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match (escape, c) {
            (Escape::Backslash, '\\') | (Escape::Backtick, '`') if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            (Escape::Backslash, '$') if chars.peek() == Some(&'\'') => {
                chars.next();
                while let Some(inner) = chars.next() {
                    match inner {
                        '\'' => break,
                        '\\' => text.extend(ansi_c_char(&mut chars)),
                        other => text.push(other),
                    }
                }
            }
            _ => text.push(c),
        }
    }
    text
}

/// Why the shell command `text` (as one line, `line`) is refused, if it is, with `found` the
/// first held file its words name from the folders it may be in and `hits` the words that name
/// one. `stetdoc mcp` and `init` are refused outright, and a `stetdoc` form that writes a held
/// file (its default output included) is refused. Else the name backstop: a command naming a
/// held file of `folder`, found by its words' paths or by its text and words ([`named_in`]), is
/// refused unless the whole command only reads ([`read_only`]).
pub(super) fn shell_refusal(
    text: &str,
    line: &str,
    escape: Escape,
    folder: &Path,
    found: Option<MatterPath>,
    hits: &[String],
) -> Option<Refusal> {
    let held = held_files(folder);
    let mut segments = Segments::new(MAX_WORDS);
    shell_segments(text, escape, &mut segments);
    for segment in segments.iter().filter(|segment| segment.command) {
        match stetdoc(&segment.words, &held, hits) {
            Stetdoc::Serve => return Some(Refusal::StetdocServe),
            Stetdoc::Writes(Some(file)) => {
                return Some(Refusal::HeldOpen(file.path.clone()));
            }
            Stetdoc::Writes(None) | Stetdoc::Fine | Stetdoc::Unknown => {}
        }
    }
    let path = found.or_else(|| named_in(text, escape, &held).map(|file| file.path.clone()))?;
    let mut names_held = held;
    if !names_held.iter().any(|file| file.path == path) {
        names_held.push(Held::new(path.clone()));
    }
    (!read_only(line, escape, &names_held, hits)).then_some(Refusal::HeldOpen(path))
}

/// A program word's name: its last path part, lower case, without `.exe`.
fn program_name(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

#[cfg(test)]
mod tests;
