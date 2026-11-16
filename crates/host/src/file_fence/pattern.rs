//! The fence file's patterns, matched against a resolved path's names under a fenced root, and
//! the base folder of a `Glob` pattern.
//!
//! A pattern is Claude Code's rule form under a root (`data_fence::DENIED`, the launch's
//! enumeration): `/`-separated names, `**` alone any number of names (none too), `*` any run of
//! characters in one name, `?` one character, `\` making the next character plain. A `[` that
//! is not escaped (the host never writes one) counts as `?`, the wider match. A path is fenced
//! when the pattern matches its names or the names of a folder above it: what is under a denied
//! folder is denied too.

use std::ffi::OsString;

use crate::network_paths::fold_name;

/// One piece of a pattern's name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// Characters that must be there, folded when the match ignores case.
    Text(String),
    /// `*`: any run of characters.
    Any,
    /// `?` (or an unescaped `[…]`): one character.
    One,
}

/// One name of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Name {
    /// `**`: any number of names, none included.
    Deep,
    /// A name to match.
    Glob(Vec<Piece>),
}

/// A pattern read for matching, its text folded when `insensitive`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pattern(Vec<Name>);

impl Pattern {
    /// `text` read as a pattern; characters folded ([`fold_name`]) when `insensitive`.
    pub(crate) fn new(text: &str, insensitive: bool) -> Self {
        let names = text
            .split('/')
            .filter(|name| !name.is_empty())
            .map(|name| {
                if name == "**" {
                    Name::Deep
                } else {
                    Name::Glob(pieces(name, insensitive))
                }
            })
            .collect();
        Self(names)
    }

    /// Whether the pattern matches `names` or the names of a folder above them (a prefix).
    pub(crate) fn covers(&self, names: &[String]) -> bool {
        covers(&self.0, names)
    }

    /// Whether something at or under `names` (a folder to search) may be one the pattern
    /// [`Self::covers`]: `names` are a start the pattern's names can go on from, or the pattern
    /// covers them already.
    pub(crate) fn may_hold(&self, names: &[String]) -> bool {
        may_hold(&self.0, names)
    }
}

/// `name` split into pieces, its text folded when `insensitive`.
fn pieces(name: &str, insensitive: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = name.chars();
    let flush = |text: &mut String, out: &mut Vec<Piece>| {
        if !text.is_empty() {
            let taken = std::mem::take(text);
            out.push(Piece::Text(if insensitive {
                fold_name(&taken)
            } else {
                taken
            }));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => text.extend(chars.next()),
            '*' => {
                flush(&mut text, &mut out);
                if out.last() != Some(&Piece::Any) {
                    out.push(Piece::Any);
                }
            }
            '?' => {
                flush(&mut text, &mut out);
                out.push(Piece::One);
            }
            '[' => {
                flush(&mut text, &mut out);
                // A class the host never writes: one character, whatever it holds.
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                }
                out.push(Piece::One);
            }
            _ => text.push(c),
        }
    }
    flush(&mut text, &mut out);
    out
}

/// Whether the pieces match the whole of `name`'s characters.
fn name_matches(pieces: &[Piece], name: &[char]) -> bool {
    match pieces.split_first() {
        None => name.is_empty(),
        Some((Piece::Text(text), rest)) => {
            let want: Vec<char> = text.chars().collect();
            name.starts_with(&want) && name_matches(rest, name.get(want.len()..).unwrap_or(&[]))
        }
        Some((Piece::One, rest)) => {
            !name.is_empty() && name_matches(rest, name.get(1..).unwrap_or(&[]))
        }
        Some((Piece::Any, rest)) => {
            (0..=name.len()).any(|skip| name_matches(rest, name.get(skip..).unwrap_or(&[])))
        }
    }
}

/// Whether the glob name matches `name`.
fn glob_matches(pieces: &[Piece], name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    name_matches(pieces, &chars)
}

/// [`Pattern::covers`] on the names.
fn covers(pattern: &[Name], names: &[String]) -> bool {
    match pattern.split_first() {
        // The pattern is used up: these names are under what it matched (or are it).
        None => true,
        Some((Name::Deep, rest)) => {
            covers(rest, names)
                || (!names.is_empty() && covers(pattern, names.get(1..).unwrap_or(&[])))
        }
        Some((Name::Glob(pieces), rest)) => names
            .split_first()
            .is_some_and(|(first, after)| glob_matches(pieces, first) && covers(rest, after)),
    }
}

/// [`Pattern::may_hold`] on the names.
fn may_hold(pattern: &[Name], names: &[String]) -> bool {
    let Some((first, after)) = names.split_first() else {
        // The folder's names are used up: a name below it can go on with the pattern.
        return true;
    };
    match pattern.split_first() {
        None => true,
        Some((Name::Deep, rest)) => may_hold(rest, names) || may_hold(pattern, after),
        Some((Name::Glob(pieces), rest)) => glob_matches(pieces, first) && may_hold(rest, after),
    }
}

/// `names` folded for a match when `insensitive` ([`fold_name`]), lossily made text.
pub(crate) fn folded(names: &[OsString], insensitive: bool) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            let text = name.to_string_lossy();
            if insensitive {
                fold_name(&text)
            } else {
                text.into_owned()
            }
        })
        .collect()
}

/// Where a `Glob` pattern starts: its leading names that hold no glob character, as text (the
/// separators as written), and the rest. `None` when the rest may reach folders its start does
/// not hold, which the fence can't bound: a `..` name, a brace group spanning a separator or
/// holding a `..` (`{/a,/b}`), or, with Windows' rules, a `:` (another drive).
pub(crate) fn glob_base(pattern: &str, windows: bool) -> Option<(String, String)> {
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let meta = |name: &str| {
        name.contains(['*', '?', '[', ']', '{', '}']) || (!windows && name.contains('\\'))
    };
    let mut end = 0;
    let mut start = 0;
    for (index, c) in pattern.char_indices().chain([(pattern.len(), '/')]) {
        if separator(c) {
            let name = pattern.get(start..index).unwrap_or_default();
            if meta(name) {
                break;
            }
            end = index;
            start = index.saturating_add(c.len_utf8());
        }
    }
    let (base, rest) = pattern.split_at(end);
    let braced = rest.split('{').skip(1).any(|group| {
        group
            .split('}')
            .next()
            .is_some_and(|inside| inside.contains(separator) || inside.contains(".."))
    });
    let unbounded =
        rest.split(separator).any(|name| name == "..") || braced || (windows && rest.contains(':'));
    (!unbounded).then(|| (base.to_owned(), rest.to_owned()))
}
