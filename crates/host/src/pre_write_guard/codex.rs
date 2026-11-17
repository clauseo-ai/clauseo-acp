//! The Codex side of the guard: a patch's targets, and the matter's `.codex` and `.agents`
//! folders (clauseo-acp-private#184): Codex loads project config from the first and skills from
//! the second (#1159 refuses Codex a matter holding `.agents`). A `Bash` word
//! naming either is refused whether it reads or writes, as `.codex` always was: the guard can't
//! tell a command's reads from its writes by its words.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Reach, lexical};
use crate::network_paths::name_folds_to;

/// Whether `path`, taken from `base`, is `<folder>/.codex` or `<folder>/.agents` or lies under
/// one, resolved by the network-safe walk (never when it is or leads to a network location,
/// [`Reach::walked`], its walked path used as it is) or lexically.
pub(super) fn in_codex_folder(folder: &Path, base: &Path, path: &Path, reach: &Reach) -> bool {
    let absolute = base.join(path);
    if let Some(walked) = reach.walked(&absolute)
        && let Some(inside) = clauseo_matters::matter_path_of_resolved(folder, &walked)
        && inside.as_str().split('/').next().is_some_and(is_codex)
    {
        return true;
    }
    lexical(&absolute)
        .strip_prefix(folder)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str().to_str().is_some_and(is_codex))
}

/// The folder names the guard keeps Codex out of in a matter.
const GUARDED: [&str; 2] = [".codex", ".agents"];

/// Whether `name` is `.codex` or `.agents` ([`GUARDED`]), ignoring trailing dots and spaces
/// and case, folded as a case-insensitive volume folds names ([`name_folds_to`]: a refusal, so
/// over-matching is safe).
fn is_codex(name: &str) -> bool {
    let name = OsStr::new(name.trim_end_matches(['.', ' ']));
    GUARDED.iter().any(|guarded| name_folds_to(name, guarded))
}

/// Whether any part of `path`, split at `/` and `\\`, is `.codex` or `.agents` ([`is_codex`]).
pub(super) fn has_codex_part(path: &Path) -> bool {
    path.to_str().is_some_and(text_has_codex_part)
}

/// Whether any part of `text`, split at `/` and `\\`, is `.codex` or `.agents` ([`is_codex`]),
/// in any of its brace forms; true too for a word whose forms run past [`MAX_FORMS`], whose
/// later forms are never looked at (a refusal, so over-matching is safe).
pub(super) fn text_has_codex_part(text: &str) -> bool {
    let mut texts = Vec::new();
    let complete = brace_forms(text, &mut texts);
    !complete
        || texts
            .iter()
            .any(|text| text.split(['/', '\\']).any(is_codex))
}

/// How many forms [`brace_forms`] gives a word at most; past it the word is cut off.
const MAX_FORMS: usize = 64;

/// `text` and, for each `{a,b,…}` group or `{x..y[..step]}` range in it, every form a shell's
/// brace expansion gives (`.agent{s,}` is `.agents` and `.agent`, `.agent{r..t}` is
/// `.agentr`, `.agents` and `.agentt`), groups nested or in a row, up to [`MAX_FORMS`].
/// False when the forms were cut off there, or a group looks like a range ([`range`]) this
/// can't expand exactly as bash does.
fn brace_forms(text: &str, out: &mut Vec<String>) -> bool {
    if out.len() >= MAX_FORMS {
        return false;
    }
    out.push(text.to_owned());
    let (open, close, alternatives) = match first_group(text) {
        Group::None => return true,
        Group::Unknown => return false,
        Group::Found(open, close, alternatives) => (open, close, alternatives),
    };
    let (Some(before), Some(after)) = (text.get(..open), text.get(close + 1..)) else {
        return true;
    };
    alternatives
        .into_iter()
        .all(|alternative| brace_forms(&format!("{before}{alternative}{after}"), out))
}

/// What [`first_group`] finds in a word.
enum Group {
    /// No group to expand: the word stands as written.
    None,
    /// A group that looks like a range but is not one expanded exactly ([`range`]).
    Unknown,
    /// Where the group opens and closes, and its forms.
    Found(usize, usize, Vec<String>),
}

/// The first `{…}` group of `text` holding a `,` at its own depth, or a range `{x..y[..step]}`
/// with no braces inside: where it opens and closes, and its forms. Unknown when a group is
/// found in a word whose `{` and `}` don't pair up, or a `{…}` with no `,` or `..` closes
/// with a `,` and a `}` still after it: the words come with quotes and backslashes taken out,
/// so a `{` or `}` here may have been quoted (`.agent{'x}',s}`), and bash reads on past a
/// `}` until it has seen a `,` (`.agent{x},s}` is `.agentx}` and `.agents`).
fn first_group(text: &str) -> Group {
    match first_group_as_written(text) {
        Group::Found(..) if text.matches('{').count() != text.matches('}').count() => {
            Group::Unknown
        }
        group @ (Group::None | Group::Unknown | Group::Found(..)) => group,
    }
}

/// [`first_group`] taking each `{` and `}` as written.
fn first_group_as_written(text: &str) -> Group {
    for (open, _) in text.match_indices('{') {
        let mut depth = 0usize;
        let mut cuts = vec![open + 1];
        let Some(rest) = text.get(open..) else {
            continue;
        };
        for (at, c) in rest.char_indices() {
            let at = open + at;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if cuts.len() < 2 {
                            let inner = text.get(open + 1..at).unwrap_or_default();
                            if inner.contains("..") && !inner.contains(['{', '}']) {
                                match range(inner) {
                                    Range::Literal => {}
                                    Range::Unknown => return Group::Unknown,
                                    Range::Forms(forms) => return Group::Found(open, at, forms),
                                }
                            } else if !inner.contains("..")
                                && text
                                    .get(at + 1..)
                                    .is_some_and(|rest| rest.contains(',') && rest.contains('}'))
                            {
                                return Group::Unknown;
                            }
                            break;
                        }
                        cuts.push(at + 1);
                        let alternatives = cuts
                            .windows(2)
                            .filter_map(|pair| match pair {
                                [start, end] => text.get(*start..end.saturating_sub(1)),
                                _ => None,
                            })
                            .map(str::to_owned)
                            .collect();
                        return Group::Found(open, at, alternatives);
                    }
                }
                ',' if depth == 1 => cuts.push(at + 1),
                _ => {}
            }
        }
    }
    Group::None
}

/// What a `{…}` group with `..` and no `,` is to [`range`].
enum Range {
    /// Not a range: some part is no range end (`{print $1".."$2}`); bash leaves it as written.
    Literal,
    /// Every part looks like a range end, but this does not expand it exactly: `+1`, a step
    /// of 0, ends of mixed kinds, a non-ASCII character, more than three parts, or more than
    /// [`MAX_FORMS`] forms.
    Unknown,
    /// The forms, in order.
    Forms(Vec<String>),
}

/// A bash sequence expression's inside, `x..y` or `x..y..step`: `x` and `y` both integers
/// (zero-padded to the longer one's width when either starts with `0` or `-0` and has more
/// than one digit) or both single ASCII characters other than digits, from `x` to `y` by the
/// step's size (1 when absent). Unknown when it starts with `...`: zsh reads `{...s}` as `.`
/// to `s`, where bash leaves it as written.
fn range(inner: &str) -> Range {
    if inner.starts_with("...") {
        return Range::Unknown;
    }
    let parts: Vec<&str> = inner.split("..").collect();
    let like_end = |part: &&str| {
        let digits = part.strip_prefix(['-', '+']).unwrap_or(part);
        part.chars().count() == 1
            || (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    };
    if !parts.iter().all(like_end) {
        return Range::Literal;
    }
    sequence(&parts).map_or(Range::Unknown, Range::Forms)
}

/// [`range`]'s forms for `parts`, each looking like a range end; None when not exact.
fn sequence(parts: &[&str]) -> Option<Vec<String>> {
    let (from, to, step) = match parts {
        [from, to] => (*from, *to, 1),
        [from, to, step] => (*from, *to, integer(step)?),
        _ => return None,
    };
    let step = step.unsigned_abs();
    if step == 0 {
        return None;
    }
    let (start, end, width) = match (integer(from), integer(to)) {
        (Some(start), Some(end)) => {
            let padded = |text: &str| {
                let digits = text.strip_prefix('-').unwrap_or(text);
                digits.starts_with('0') && digits.len() > 1
            };
            let width = if padded(from) || padded(to) {
                from.len().max(to.len())
            } else {
                0
            };
            (start, end, Some(width))
        }
        (None, None) => (single_ascii(from)?, single_ascii(to)?, None),
        _ => return None,
    };
    let count = start.abs_diff(end) / step + 1;
    if count > MAX_FORMS as u64 {
        return None;
    }
    let down = end < start;
    (0..count)
        .map(|at| {
            let offset = i64::try_from(at * step).ok()?;
            let value = if down {
                start.checked_sub(offset)?
            } else {
                start.checked_add(offset)?
            };
            Some(match width {
                Some(width) if value < 0 => {
                    let pad = width.saturating_sub(1);
                    format!("-{:0>pad$}", value.unsigned_abs())
                }
                Some(width) => format!("{value:0>width$}"),
                None => char::from(u8::try_from(value).ok()?).to_string(),
            })
        })
        .collect()
}

/// `text` as a bash range end that is an integer: digits with an optional leading `-`.
fn integer(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `text` as a bash range end that is one character: a single ASCII character, not a digit.
fn single_ascii(text: &str) -> Option<i64> {
    match text.as_bytes() {
        [byte] if byte.is_ascii() && !byte.is_ascii_digit() => Some(i64::from(*byte)),
        _ => None,
    }
}

/// The files a Codex `apply_patch` patch adds, updates, deletes or moves to.
pub(super) fn patch_targets(patch: &str) -> Vec<PathBuf> {
    const HEADERS: [&str; 4] = [
        "*** Add File:",
        "*** Update File:",
        "*** Delete File:",
        "*** Move to:",
    ];
    patch
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            HEADERS
                .iter()
                .find_map(|header| line.strip_prefix(header))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// `.codex` is refused in every case and with trailing dots and spaces (it has no `s` or
    /// `k` for the long s or the Kelvin sign to stand for).
    #[test]
    fn a_codex_folder_is_found_in_any_folded_spelling() {
        for name in [".codex", ".CODEX", ".CoDeX", ".codex. "] {
            assert!(super::is_codex(name), "{name}");
        }
        assert!(!super::is_codex(".codexx"));
        for name in [".agents", ".AGENTS", ".Agents. "] {
            assert!(super::is_codex(name), "{name}");
        }
        assert!(!super::is_codex(".agent"));
        assert!(!super::is_codex("agents"));
    }

    /// A brace group's forms are each checked: `.agent{s,}` names `.agents`.
    #[test]
    fn a_brace_form_naming_a_guarded_folder_is_found() {
        for word in [
            ".agent{s,}/skills/x",
            "{x,.agents}/y",
            ".{agents,codex}",
            "a/.age{n{t,x},y}s/b",
        ] {
            assert!(super::text_has_codex_part(word), "{word}");
        }
        for word in ["{a,b}/c", ".agent{x,y}", "{.agentsrc,z}", "{}", "{,}"] {
            assert!(!super::text_has_codex_part(word), "{word}");
        }
    }

    /// A word whose brace forms run past the limit is refused: its last form, `.agents`, is
    /// never reached.
    #[test]
    fn a_brace_word_cut_off_at_the_limit_is_refused() {
        let mut alternatives: Vec<String> = (0..64).map(|at| format!("x{at}")).collect();
        alternatives.push("n".to_owned());
        let word = format!(".age{{{}}}ts/skills", alternatives.join(","));
        assert!(super::text_has_codex_part(&word), "{word}");
    }

    /// A range's forms are each checked: `.agent{r..t}` names `.agents`; a numeric range that
    /// multiplies a `.codex` form past the limit is refused; an ordinary range passes, and a
    /// group holding `..` that is no range expanded exactly is refused.
    #[test]
    fn a_range_naming_a_guarded_folder_is_found() {
        for word in [
            ".agent{r..t}/skills/x",
            ".agent{t..r}",
            ".{a..c}gents",
            ".agent{a..z..2}",
            "{.codex,x}{1..40}",
            "{1..2..0}",
            ".agent{é..z}",
        ] {
            assert!(super::text_has_codex_part(word), "{word}");
        }
        for word in [
            "file{1..3}.txt",
            "v{01..10}",
            "{-2..2..2}",
            ".agent{a..q}",
            "x{y}z",
            "{print $1\"..\"$2}",
            ".agent{a..zz}",
        ] {
            assert!(!super::text_has_codex_part(word), "{word}");
        }
    }

    /// Range forms as bash gives them: zero-padded to the wider end, by the step's size in the
    /// ends' direction.
    #[test]
    fn ranges_expand_as_bash_does() {
        let forms = |inner: &str| match super::range(inner) {
            super::Range::Forms(forms) => forms.join(" "),
            super::Range::Unknown => "unknown".to_owned(),
            super::Range::Literal => "literal".to_owned(),
        };
        assert_eq!(forms("1..3"), "1 2 3");
        assert_eq!(forms("01..10..3"), "01 04 07 10");
        assert_eq!(forms("5..1..-2"), "5 3 1");
        assert_eq!(forms("-05..5..5"), "-05 000 005");
        assert_eq!(forms("a..e..2"), "a c e");
        assert_eq!(forms("Z..a"), "Z [ \\ ] ^ _ ` a");
        assert_eq!(forms("1..a"), "unknown");
        assert_eq!(forms("1..100"), "unknown");
        assert_eq!(forms("+1..3"), "unknown");
        assert_eq!(forms("print $1\"..\"$2"), "literal");
        assert_eq!(forms("a..zz"), "literal");
        assert_eq!(forms("...s"), "unknown");
    }

    /// A range starting with `...` is refused: zsh, the Mac's default shell, reads `{...s}` as
    /// `.` to `s`, and `.agent{...s}` gives `.agents`.
    #[test]
    fn a_range_starting_with_three_dots_is_refused() {
        for word in [
            ".agent{...s}",
            ".codex{.../}",
            ".codex{...0}/k",
            "x/.agent{...é}/k",
        ] {
            assert!(super::text_has_codex_part(word), "{word}");
        }
    }

    /// A group the shell may close later than the first `}` is refused: a quoted or escaped
    /// brace (`.agent{'x}',s}`, `.agent{x\},s}`, `.agent{'{',s}`, seen here with the quotes
    /// taken out), or bash reading past a `}` before any `,` (`.agent{x},s}`). Ordinary
    /// braces still pass.
    #[test]
    fn a_group_closed_early_is_refused() {
        for word in [".agent{x},s}", ".agent{{,s}"] {
            assert!(super::text_has_codex_part(word), "{word}");
        }
        for word in ["file{1,2}.txt", "a{b}c", "file{1..3}.txt", "{a,b}{c,d}"] {
            assert!(!super::text_has_codex_part(word), "{word}");
        }
    }
}
