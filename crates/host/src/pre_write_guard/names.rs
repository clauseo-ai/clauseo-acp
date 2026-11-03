//! The name backstop (clauseo-acp-private#162, review rounds 2 and 3): a shell command whose text
//! or words name a file of the matter that is held open right now is refused, whatever folder
//! games it plays, unless the whole command only reads ([`read_only`]); and `stetdoc`'s writing
//! and serving forms ([`stetdoc()`]).

mod allow;
mod patterns;
mod stetdoc;

use std::cell::{Cell, OnceCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clauseo_protocol::MatterPath;
use unicode_normalization::UnicodeNormalization;

use super::shell::script;
use super::words::{Escape, Segments, ansi_c_char, candidates, shell_segments};
use super::{MAX_WORDS, Refusal, timings};

pub(super) use allow::{read_only, textutil_writes};
pub(super) use stetdoc::{Stetdoc, stetdoc};

/// The most folder entries [`held_files`] reads; a larger matter is read in part, shallow folders
/// first.
const MAX_ENTRIES: usize = 20_000;

/// The most words [`read_only`] and [`named_in`] read; a longer command is not read-only.
const MAX_READ_WORDS: usize = 4096;

/// A file of the matter that is held open, or (on Windows) may be: a file beside an owner file
/// that points at another, which only opening it can tell, opened the first time a command's
/// text or wildcard matches its name ([`Held::is_held`]).
#[derive(Debug, Clone)]
pub(super) struct Held {
    pub(super) path: MatterPath,
    /// Its last part folded as [`fold`] does, and as stored, only case-folded ([`case`]): a `?`
    /// wildcard counts characters, which composing changes, so wildcards are matched against
    /// both. Made on first use.
    names: OnceCell<(String, String)>,
    state: Cell<State>,
    /// Where to look when `state` is [`State::Unknown`].
    folder: Option<Rc<Listed>>,
}

/// Whether a [`Held`] file is held open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Held,
    NotHeld,
    /// Not looked at yet.
    Unknown,
}

/// A listed folder with an owner file: the matter's canonical root and the folder's owner files,
/// for [`clauseo_matters::held_open_beside`].
#[derive(Debug)]
struct Listed {
    root: PathBuf,
    owners: Vec<String>,
}

impl Held {
    /// The held file at `path`.
    pub(super) fn new(path: MatterPath) -> Self {
        Self {
            path,
            names: OnceCell::new(),
            state: Cell::new(State::Held),
            folder: None,
        }
    }

    /// The file at `path` in `folder`, held or not as [`clauseo_matters::held_open_beside`]
    /// will say when asked.
    fn unknown(path: MatterPath, folder: &Rc<Listed>) -> Self {
        Self {
            path,
            names: OnceCell::new(),
            state: Cell::new(State::Unknown),
            folder: Some(Rc::clone(folder)),
        }
    }

    /// Its last part, folded ([`fold`]), and as stored, case-folded ([`case`]).
    fn names(&self) -> &(String, String) {
        self.names.get_or_init(|| {
            let last = self.path.as_str().rsplit('/').next().unwrap_or_default();
            (fold(last), case(last))
        })
    }

    /// Whether it is held open: known, or asked of [`clauseo_matters::held_open_beside`] once,
    /// the answer kept.
    fn is_held(&self) -> bool {
        match self.state.get() {
            State::Held => true,
            State::NotHeld => false,
            State::Unknown => {
                timings::probed();
                let held = self.folder.as_ref().is_some_and(|folder| {
                    clauseo_matters::held_open_beside(&folder.root, &self.path, &folder.owners)
                        .is_some()
                });
                self.state
                    .set(if held { State::Held } else { State::NotHeld });
                held
            }
        }
    }
}

/// Two are the same file when their paths are.
impl PartialEq for Held {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Held {}

/// Whether a file that no owner file beside it points at may still be held open, so the walk
/// keeps it to open when a command names it ([`Held::is_held`]): on Windows, where
/// [`clauseo_matters::held_open_beside`] also opens the file and a sharing violation counts.
const SHARE_MODES: bool = cfg!(windows);

/// The files of the matter `folder` held open by an Office owner file
/// ([`clauseo_matters::held_open_beside`], in each folder that has an owner file `~$…`, given
/// that folder's owner files from this one listing), read breadth first: no folder whose name
/// starts with `.`, no symlink followed, at most [`MAX_ENTRIES`] entries. Each folder is listed
/// once and its names matched against its owner files in memory
/// ([`clauseo_matters::OwnersBeside`]); only a file one may point at is looked up, so a folder of
/// thousands of files beside one owner file costs its listing and a few calls. On Windows a file
/// beside an owner file that points at another is kept unknown, opened only when a command's
/// text or wildcard matches its name ([`SHARE_MODES`]): the lookup there also finds a file a
/// program holds by a share mode alone, and this keeps finding it in such a folder. A file so held
/// in a folder with no owner file is not found here.
pub(super) fn held_files(folder: &Path) -> Vec<Held> {
    held_files_with(folder, SHARE_MODES)
}

/// [`held_files`], keeping the files no owner file points at to open later when `share_modes`.
fn held_files_with(folder: &Path, share_modes: bool) -> Vec<Held> {
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut held = Vec::new();
    let mut queue: std::collections::VecDeque<PathBuf> = std::iter::once(PathBuf::new()).collect();
    let mut walk = timings::Walk::default();
    while let Some(relative) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(root.join(&relative)) else {
            continue;
        };
        walk.folders += 1;
        let mut files = Vec::new();
        let mut owners: Vec<String> = Vec::new();
        // Every `~$` entry, of any kind: the lookup's stats find a folder of that name too.
        let mut tilde: Vec<String> = Vec::new();
        for entry in read.flatten() {
            walk.entries += 1;
            if walk.entries > MAX_ENTRIES {
                walk.entries = MAX_ENTRIES;
                timings::walked(walk);
                return held;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with("~$") {
                tilde.push(name.clone());
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') {
                    queue.push_back(relative.join(&name));
                }
            } else if name.starts_with("~$") {
                owners.push(name);
            } else {
                files.push(name);
            }
        }
        if owners.is_empty() {
            continue;
        }
        walk.owner_folders += 1;
        walk.checked += files.len();
        let beside = clauseo_matters::OwnersBeside::new(&tilde);
        let listed = Rc::new(Listed {
            root: root.clone(),
            owners,
        });
        for name in files {
            let candidate = beside.may_hold(&name);
            if !candidate && !share_modes {
                continue;
            }
            let Some(path) = MatterPath::from_relative(&relative.join(&name)) else {
                continue;
            };
            if candidate {
                walk.probed += 1;
                if clauseo_matters::held_open_beside(&root, &path, &listed.owners).is_some() {
                    held.push(Held::new(path));
                }
            } else {
                held.push(Held::unknown(path, &listed));
            }
        }
    }
    timings::walked(walk);
    held
}

/// `text` as the safety matches compare names (decision 0007, Unicode names): composed (NFC)
/// everywhere, so a held name typed in the other Unicode form is caught (APFS finds it; ext4
/// and NTFS would make another file, and refusing is the safe side), and lower case on macOS
/// and Windows, whose file systems ignore case.
fn fold(text: &str) -> String {
    case(text).nfc().collect()
}

/// `text` lower case on macOS and Windows, whose file systems ignore case; as it is elsewhere.
fn case(text: &str) -> String {
    if cfg!(any(target_os = "macos", windows)) {
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
    held.iter()
        .find(|file| holds(&text, &file.names().0) && file.is_held())
}

/// The first of `held` whose name a wildcard in the shell word `word` could match: the last
/// part (after `/` or `\`) of `word` or of one of its [`candidates`], when it has `*`, `?` or
/// `[`, matched against each held name in any folder ([`glob_matches`]). A batch command such
/// as `cp Revised/*.docx .` or `for f in *.docx` so counts as naming the held file; a wildcard
/// in quotes counts too (the words come unquoted), a refusal on the safe side.
fn globbed<'a>(word: &str, held: &'a [Held]) -> Option<&'a Held> {
    candidates(word).find_map(|candidate| {
        let last = candidate.rsplit(['/', '\\']).next().unwrap_or(candidate);
        if !last.contains(['*', '?', '[']) {
            return None;
        }
        let patterns: [Vec<char>; 2] = [fold(last).chars().collect(), case(last).chars().collect()];
        held.iter().find(|file| {
            let (name, stored) = file.names();
            let names: [Vec<char>; 2] = [name.chars().collect(), stored.chars().collect()];
            patterns
                .iter()
                .any(|pattern| names.iter().any(|name| glob_matches(pattern, name)))
                && file.is_held()
        })
    })
}

/// Whether the shell wildcard `pattern` matches all of `name`: `*` any run, `?` any one
/// character, `[…]` one of a set (ranges `a-z`, `!` or `^` first negates; an unclosed `[` is
/// itself); a leading `.` is not matched by a wildcard, as the shell does. Linear backtracking
/// on the last `*`, so a long pattern costs its length times the name's.
fn glob_matches(pattern: &[char], name: &[char]) -> bool {
    if name.first() == Some(&'.') && pattern.first() != Some(&'.') {
        return false;
    }
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while let Some(&c) = name.get(n) {
        let rest = pattern.get(p..).unwrap_or_default();
        let step = match rest.first() {
            Some('*') => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some('?') => Some(1),
            Some('[') => class(rest, c),
            Some(&literal) => (literal == c).then_some(1),
            None => None,
        };
        match (step, star) {
            (Some(width), _) => {
                p += width;
                n += 1;
            }
            (None, Some((at, from))) => {
                p = at + 1;
                n = from + 1;
                star = Some((at, from + 1));
            }
            (None, None) => return false,
        }
    }
    pattern
        .get(p..)
        .unwrap_or_default()
        .iter()
        .all(|c| *c == '*')
}

/// For a pattern starting at `[`: how many pattern characters its class takes when it matches
/// `c`; `None` when it does not. An unclosed `[` is a plain character.
fn class(pattern: &[char], c: char) -> Option<usize> {
    let negated = matches!(pattern.get(1), Some('!' | '^'));
    let start = if negated { 2 } else { 1 };
    // A `]` right after the opening (or the negation) is a member, not the end.
    let Some(end) = pattern
        .iter()
        .skip(start + 1)
        .position(|ch| *ch == ']')
        .map(|at| at + start + 1)
    else {
        return (c == '[').then_some(1);
    };
    let set = pattern.get(start..end).unwrap_or_default();
    let mut found = false;
    let mut members = set.iter().peekable();
    while let Some(&low) = members.next() {
        let mut ahead = members.clone();
        if ahead.next() == Some(&'-')
            && let Some(&high) = ahead.next()
        {
            found |= (low..=high).contains(&c);
            members = ahead;
        } else {
            found |= low == c;
        }
    }
    (found != negated).then_some(end + 1)
}

/// How a shell word names a held file ([`names_word`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named<'a> {
    /// By name ([`named`]) or by a wildcard that could match it ([`globbed`]).
    File(&'a Held),
    /// As a word the path layer found resolving to a held file (`hits`), or a part of one.
    Hit,
}

impl<'a> Named<'a> {
    /// The held file, when the word names it by name.
    fn file(self) -> Option<&'a Held> {
        match self {
            Self::File(file) => Some(file),
            Self::Hit => None,
        }
    }
}

/// What the word `word` of a command (not a heredoc body) names: a held file by name or
/// wildcard, or a path-layer hit or a part of one ([`candidates`]: `Link.docx` of a hit
/// `--output=Link.docx`).
fn names_word<'a>(word: &str, held: &'a [Held], hits: &[String]) -> Option<Named<'a>> {
    named(word, held)
        .or_else(|| globbed(word, held))
        .map(Named::File)
        .or_else(|| {
            hits.iter()
                .any(|hit| hit == word || candidates(hit).any(|part| part == word))
                .then_some(Named::Hit)
        })
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
    let searching = patterns::only_searches(&segments.iter().collect::<Vec<_>>());
    segments.iter().find_map(|segment| {
        segment
            .words
            .iter()
            .enumerate()
            .find_map(|(at, word)| {
                named(word, held).or_else(|| {
                    globbed(word, held).filter(|_| {
                        segment.command && !(searching && patterns::quoted_pattern(segment, at))
                    })
                })
            })
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
/// one. `stetdoc mcp` and `init` are refused outright, and a `stetdoc` or `textutil` form that
/// writes a held file (its default output included) is refused. Else the name backstop: a
/// command naming a held file of `folder`, found by its words' paths or by its text and words
/// ([`named_in`]), is refused unless the whole command only reads ([`read_only`]).
pub(super) fn shell_refusal(
    text: &str,
    line: &str,
    escape: Escape,
    folder: &Path,
    found: Option<MatterPath>,
    hits: &[String],
) -> Option<Refusal> {
    let held = held_files(folder);
    super::timings::mark("walk");
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
        if let Some(file) = textutil_writes(&segment.words, &held) {
            return Some(Refusal::HeldOpen(file.path.clone()));
        }
    }
    let path = found.or_else(|| named_in(text, escape, &held).map(|file| file.path.clone()))?;
    let mut names_held = held;
    match names_held.iter().find(|file| file.path == path) {
        Some(file) => file.state.set(State::Held),
        None => names_held.push(Held::new(path.clone())),
    }
    (!read_only(line, escape, &names_held, hits, folder)).then_some(Refusal::HeldOpen(path))
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
