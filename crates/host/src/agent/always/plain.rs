//! Whether a command a card shows is **one plain command of a listed program**, the only kind
//! "Always for this matter" keeps (decision record 0063). Pure text work: no file system, no
//! clock; the real paths are checked on the matter thread (`reach.rs`).
//!
//! - The text is read the way the shell that runs it would read it, by a grammar much smaller
//!   than the shell's ([`Grammar`]): every word a plain literal of a small set of characters,
//!   quoted or not. Anything else fails closed: a chain, a variable, a substitution, a glob, a
//!   typographic quote, an invisible character.
//! - The program is one the allowlist names (`table.rs`), as a bare name: never a path, so
//!   never a file in the matter. Its options and words must fit its entry.
//! - No path word may name a place outside the matter by its text ([`outside`]): an absolute
//!   path outside the matter folder. A `..` name never fits a path word.
//! - Codex's own shell wrapper is read through only at the exact paths Codex launches
//!   ([`own_wrapper`]): codex-acp 2.2.2 shows Codex's `[/bin/zsh, -c, script]` as
//!   `stripShellPrefix(shlex_join(argv))`, usually the script alone.

use super::table::{self, Spec, Unfit};

/// The longest command kept, in bytes.
pub(crate) const MAX_COMMAND_BYTES: usize = 8 * 1024;

/// Which shell's words the command is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grammar {
    /// A POSIX shell (Claude's `Bash`, Codex's `zsh -c` on macOS and Linux).
    Posix,
    /// PowerShell (Claude's `PowerShell`, Codex's `powershell -Command` on Windows).
    PowerShell,
}

/// Whose card it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Agent {
    /// Claude Code: its shell tools take the script as a string.
    Claude,
    /// Codex: codex-acp shows its command line, a shell wrapper usually stripped.
    Codex,
}

/// Where the command runs, for [`outside`]: the matter folder, as text. A relative path word is
/// judged by real path on the matter thread (`reach.rs`), from the working folder.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Place<'a> {
    pub(crate) matter: &'a str,
    /// Windows paths (drive letters, case folded).
    pub(crate) windows: bool,
}

/// Why a command is not one plain command of a listed program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotPlain {
    /// Empty, or longer than [`MAX_COMMAND_BYTES`].
    Size,
    /// A character or a form the grammar does not take: a chain, a pipe, a redirection, a
    /// substitution, a glob, a variable, an escape, an unclosed quote, a character outside the
    /// set, a program given as a path.
    Unparsed,
    /// A program the allowlist does not name, an option its entry does not list, or a word that
    /// does not fit it (`table.rs`).
    NotListed,
    /// A path word that names a place outside the matter.
    Outside,
}

/// One plain command of a listed program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plain {
    /// Its words, the program first.
    pub(crate) words: Vec<String>,
    /// Its program's entry.
    pub(crate) program: &'static Spec,
    /// The words its program opens as files, as typed.
    pub(crate) paths: Vec<String>,
}

/// The one plain command `line` runs, or why it has none.
pub(crate) fn plain(
    line: &str,
    grammar: Grammar,
    agent: Agent,
    place: Place<'_>,
) -> Result<Plain, NotPlain> {
    if line.is_empty() || line.len() > MAX_COMMAND_BYTES {
        return Err(NotPlain::Size);
    }
    let words = split(line, grammar)?;
    if agent == Agent::Codex
        && let Some((script, inner)) = own_wrapper(&words, place.windows)
    {
        return checked(split(&script, inner)?, inner, place);
    }
    checked(words, grammar, place)
}

fn checked(words: Vec<String>, grammar: Grammar, place: Place<'_>) -> Result<Plain, NotPlain> {
    let (program, args) = words.split_first().ok_or(NotPlain::Unparsed)?;
    if !program_name(program) {
        return Err(NotPlain::Unparsed);
    }
    let spec = table::find(program, grammar).ok_or(NotPlain::NotListed)?;
    let read = table::read(spec, args).map_err(|why| match why {
        Unfit::Option | Unfit::Word | Unfit::Count => NotPlain::NotListed,
    })?;
    if read.paths.iter().any(|word| outside(word, place)) {
        return Err(NotPlain::Outside);
    }
    Ok(Plain {
        program: spec,
        paths: read.paths,
        words,
    })
}

/// The script of Codex's own wrapper, and the grammar it is in, read through only at the paths
/// Codex itself launches: `[/bin/zsh|/bin/bash|/bin/sh, -c|-lc, script]` off Windows (Codex
/// runs the account's shell; one elsewhere is not read through), and on Windows
/// `[<System32>\WindowsPowerShell\v1.0\powershell.exe, flags…, -Command, script]` with only
/// `-NoProfile`, `-NoLogo` or `-NonInteractive` as flags. A shell by a relative path, a bare
/// name or a path in the matter is never read through, so it is a program the allowlist does not
/// name.
fn own_wrapper(words: &[String], windows: bool) -> Option<(String, Grammar)> {
    let program = words.first()?;
    if !windows {
        if !POSIX_SHELLS.contains(&program.as_str()) {
            return None;
        }
        let [_, flag, script] = words else {
            return None;
        };
        return matches!(flag.as_str(), "-c" | "-lc").then(|| (script.clone(), Grammar::Posix));
    }
    if !program
        .replace('\\', "/")
        .eq_ignore_ascii_case(WINDOWS_POWERSHELL)
    {
        return None;
    }
    let (script, rest) = words.get(1..)?.split_last()?;
    let (command, flags) = rest.split_last()?;
    let command = command.eq_ignore_ascii_case("-command");
    let flags_ok = flags.iter().all(|flag| {
        ["-noprofile", "-nologo", "-noninteractive"]
            .iter()
            .any(|known| flag.eq_ignore_ascii_case(known))
    });
    (command && flags_ok).then(|| (script.clone(), Grammar::PowerShell))
}

/// The shells whose `-c` script Codex's wrapper is read through to, at their system paths.
const POSIX_SHELLS: [&str; 3] = ["/bin/zsh", "/bin/bash", "/bin/sh"];

/// Windows PowerShell at its system path, `/`-separated.
const WINDOWS_POWERSHELL: &str = "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";

/// `line`'s words under `grammar`, or [`NotPlain::Unparsed`]. Each word is judged whole once
/// it ends ([`word_ok`]), knowing whether any of it was quoted.
fn split(line: &str, grammar: Grammar) -> Result<Vec<String>, NotPlain> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' || c == '\t' {
            end(&mut words, &mut word, &mut quoted, grammar)?;
            continue;
        }
        // PowerShell reads `''` inside quotes as one quote, and joins quoted and bare text by
        // rules of its own: a quote there opens a word and ends it.
        let whole = grammar == Grammar::Posix || word.is_none();
        let current = word.get_or_insert_with(String::new);
        match c {
            '\'' if whole => {
                quoted = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) if quoted_ok(inner, grammar) => current.push(inner),
                        _ => return Err(NotPlain::Unparsed),
                    }
                }
                let ends = chars.peek().is_none_or(|next| matches!(next, ' ' | '\t'));
                if grammar == Grammar::PowerShell && !ends {
                    return Err(NotPlain::Unparsed);
                }
            }
            '"' if grammar == Grammar::Posix => loop {
                quoted = true;
                match chars.next() {
                    Some('"') => break,
                    Some(inner) if quoted_ok(inner, grammar) => current.push(inner),
                    _ => return Err(NotPlain::Unparsed),
                }
            },
            c if bare_ok(c, grammar) => current.push(c),
            _ => return Err(NotPlain::Unparsed),
        }
    }
    end(&mut words, &mut word, &mut quoted, grammar)?;
    if words.is_empty() {
        return Err(NotPlain::Unparsed);
    }
    Ok(words)
}

/// Ends the word being read, if any: judged whole ([`word_ok`]), then added to `words`.
fn end(
    words: &mut Vec<String>,
    word: &mut Option<String>,
    quoted: &mut bool,
    grammar: Grammar,
) -> Result<(), NotPlain> {
    if let Some(done) = word.take() {
        if !word_ok(&done, *quoted, grammar) {
            return Err(NotPlain::Unparsed);
        }
        words.push(done);
    }
    *quoted = false;
    Ok(())
}

/// Whether a whole word, quoted in part or not, reads as the text it holds. PowerShell reads two
/// forms otherwise: a quoted word starting with `-` is a file name to it, never a switch, and a
/// bare word that looks like a number is one (`1kb` is 1024, `0x10` is 16, `007` is 7), so
/// such a word is taken only as a plain number with no leading zero, short enough to stay one.
fn word_ok(word: &str, quoted: bool, grammar: Grammar) -> bool {
    if grammar != Grammar::PowerShell {
        return true;
    }
    if quoted {
        return !word.starts_with('-');
    }
    let bytes = word.as_bytes();
    let numeric = matches!(bytes, [b'0'..=b'9' | b'+', ..] | [b'.', b'0'..=b'9', ..]);
    !numeric || word == "0" || plain_number(word)
}

/// `1` to `999999999`: ASCII digits, no leading zero, at most nine.
fn plain_number(word: &str) -> bool {
    word.len() <= 9 && !word.starts_with('0') && word.bytes().all(|b| b.is_ascii_digit())
}

/// A character taken outside quotes: ASCII letters and digits and a few marks; no other letter
/// or digit, so no look-alike name reaches a card. The shell reads none of these as more than
/// itself. PowerShell takes fewer: no `,` (an array), `@` (splatting), `%` or `=`.
fn bare_ok(c: char, grammar: Grammar) -> bool {
    let marks: &[char] = match grammar {
        Grammar::Posix => &['_', '-', '.', '/', ':', ',', '+', '=', '@', '%'],
        Grammar::PowerShell => &['_', '-', '.', '/', ':', '+'],
    };
    c.is_ascii_alphanumeric() || marks.contains(&c)
}

/// A character taken inside quotes: what [`bare_ok`] takes, and the space. Never a quote of
/// any kind (PowerShell ends a quoted string at a typographic one too), a `$`, a backquote,
/// a `\`, a control character or anything invisible.
fn quoted_ok(c: char, grammar: Grammar) -> bool {
    c == ' ' || bare_ok(c, grammar)
}

/// A program's name as typed: ASCII letters, digits, `.`, `_` and `-`, not starting with
/// `-` or `.`; no folder (a program in the matter is a file the agent can write) and no `=` (a
/// variable set for the command).
fn program_name(program: &str) -> bool {
    !program.starts_with(['-', '.'])
        && program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether the path word `word` names a place outside the matter by its text: an absolute path
/// not inside the matter folder, compared as text, case folded on Windows. A `..` name never
/// reaches here (`table::path_word`).
fn outside(word: &str, place: Place<'_>) -> bool {
    absolute(word, place.windows) && !inside(word, place)
}

fn absolute(path: &str, windows: bool) -> bool {
    if path.starts_with('/') {
        return true;
    }
    if !windows {
        return false;
    }
    let bytes = path.as_bytes();
    path.starts_with('\\') || matches!(bytes, [drive, b':', ..] if drive.is_ascii_alphabetic())
}

/// Whether `path`, made plain (`.` dropped, `..` taking its folder away), is the matter folder
/// or inside it.
fn inside(path: &str, place: Place<'_>) -> bool {
    let (Some(path), Some(root)) = (
        normal(path, place.windows),
        normal(place.matter, place.windows),
    ) else {
        return false;
    };
    path.strip_prefix(&root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        || root == "/"
}

fn normal(path: &str, windows: bool) -> Option<String> {
    let path = if windows {
        path.replace('\\', "/").to_lowercase()
    } else {
        path.to_owned()
    };
    let mut parts: Vec<&str> = Vec::new();
    for name in path.split('/') {
        match name {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    let lead = if path.starts_with("//") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    Some(format!("{lead}{}", parts.join("/")))
}

#[cfg(test)]
#[path = "plain_tests.rs"]
mod tests;
