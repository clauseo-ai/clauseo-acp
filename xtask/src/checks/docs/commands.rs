//! `cargo xtask` commands in the docs. Every code span and every line of a code block must parse,
//! wherever `cargo xtask` starts a command in it: after a `$` prompt, after `sudo` or `VAR=value`
//! assignments, or after `&&`, `||`, `;` or `|`. Each part between those operators is split
//! into words with `shlex`, as a shell splits it, so quotes are read (`cargo xtask "check"`,
//! `FOO="a b" cargo xtask check`), and the words parse against the clap tree in `main.rs`. A part
//! that holds `cargo xtask` but that `shlex` cannot split (a quote left open) fails. A `text`
//! code block holds output, not commands, and is not read (see `markdown::scan`).
//!
//! A span that is exactly `cargo xtask` names the tool in prose and is not read. A `<...>` word
//! (`<command>`) or a `...` word is a placeholder: in the subcommand's place the command is not
//! read; elsewhere the words before it are parsed, and an error that only says a word is missing
//! is not one, since the placeholder stands for that word.

use super::markdown::{Line, Scan};
use crate::Cli;
use crate::checks::violation;
use crate::violation::Violation;
use clap::CommandFactory;
use clap::error::{Error, ErrorKind};

/// The longest clap first line that the message repeats. A longer one is left out.
const SHORT: usize = 80;

/// The one fix for every command that does not parse, as a literal the planted cases can
/// `concat!`.
macro_rules! command_fix {
    () => {
        "correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` \
         lists its flags), or remove it"
    };
}
#[cfg(test)]
pub(super) use command_fix;

/// The one fix for every command that does not parse.
const FIX: &str = command_fix!();

/// The fix for a command line that `shlex` cannot split into words.
const UNSPLIT_FIX: &str = "close the quote or escape that is left open, or remove the command";

/// The span that names the tool in prose, not a command to run.
const TOOL: &str = "cargo xtask";

/// An xtask command found in the docs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Command {
    /// Its words as a shell splits them, starting `cargo`, `xtask`, up to the first placeholder.
    words: Vec<String>,
    /// True when a placeholder followed these words.
    placeholder: bool,
}

/// What a part of a line holds: a command, or the part itself when it holds `cargo xtask` but
/// `shlex` cannot split it.
type Found = Result<Command, String>;

/// One violation per xtask command in `file` that does not parse.
pub(super) fn problems(file: &str, scan: &Scan) -> Vec<Violation> {
    let spans = scan
        .spans
        .iter()
        .filter(|span| span.text != TOOL)
        .map(|span| (span.line, span.text.clone()));
    let code = logical_lines(&scan.code);
    spans
        .chain(code)
        .flat_map(|(line, text)| {
            commands_in(&text)
                .into_iter()
                .filter_map(move |found| match found {
                    Ok(command) => problem(file, line, &command),
                    Err(part) => Some(unsplit(file, line, &part)),
                })
        })
        .collect()
}

/// The violation for `part`, a command line that `shlex` cannot split, from `line`.
fn unsplit(file: &str, line: u32, part: &str) -> Violation {
    violation(
        file,
        line,
        format!(
            "the xtask command line `{part}` does not split into words: a quote or escape is left open"
        ),
        UNSPLIT_FIX,
    )
}

/// The lines of code blocks as logical lines: a line ending in a backslash continues onto the
/// next, and the pair is one line, with the first line's number.
fn logical_lines(lines: &[Line]) -> Vec<(u32, String)> {
    let mut logical = Vec::new();
    let mut open: Option<(u32, String)> = None;
    for line in lines {
        let text = line.text.trim();
        let (body, continues) = text
            .strip_suffix('\\')
            .map_or((text, false), |body| (body.trim_end(), true));
        let (start, mut joined) = match open.take() {
            Some((start, mut joined)) => {
                joined.push(' ');
                (start, joined)
            }
            None => (line.line, String::new()),
        };
        joined.push_str(body);
        if continues {
            open = Some((start, joined));
        } else {
            logical.push((start, joined));
        }
    }
    logical.extend(open);
    logical
}

/// The xtask commands in `text`, each part between the operators (see `parts`) split with
/// `shlex`. Each part may begin with a `$` prompt, `sudo` and `NAME=value` assignments.
fn commands_in(text: &str) -> Vec<Found> {
    parts(text)
        .into_iter()
        .filter_map(|part| match shlex::split(part) {
            Some(words) => command_of(&words).map(Ok),
            None => names_the_tool(part).then(|| Err(part.trim().to_owned())),
        })
        .collect()
}

/// The parts of `text` between the shell operators `&&`, `||`, `;` and `|`, read outside
/// quotes, backslash escapes and a `#` comment. A lone `&` (`2>&1`) is not one of them.
fn parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut word_start = true;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if escaped {
            escaped = false;
            word_start = false;
            continue;
        }
        let mut operator = 0;
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('"') | None, '\\') => escaped = true,
            (None, '\'' | '"') => quote = Some(c),
            (None, '#') if word_start => break,
            (None, ';') => operator = 1,
            (None, '|' | '&') => {
                let doubled = chars.peek().is_some_and(|(_, next)| *next == c);
                if doubled {
                    chars.next();
                    operator = 2;
                } else if c == '|' {
                    operator = 1;
                }
            }
            // Inside quotes, and any other character outside them.
            _ => {}
        }
        if operator > 0 {
            parts.push(text.get(start..at).unwrap_or_default());
            start = at + operator;
        }
        word_start = quote.is_none() && (operator > 0 || c.is_whitespace());
    }
    parts.push(text.get(start..).unwrap_or_default());
    parts
}

/// True when the words of `part`, cut at whitespace, hold `cargo` then `xtask`.
fn names_the_tool(part: &str) -> bool {
    let words: Vec<&str> = part.split_whitespace().collect();
    words.windows(2).any(|pair| {
        pair.first().map(|w| w.trim_start_matches(['(', '$'])) == Some("cargo")
            && pair.get(1) == Some(&"xtask")
    })
}

/// The command `words`, a part of a line split by `shlex`, hold, when they hold one (see
/// `commands_in`). Before `cargo`, a `(` or `$(`, a `sudo`, `time`, `env` or `exec`, and
/// assignments are skipped, and a `+<toolchain>` right after `cargo` is skipped too
/// (`cargo +nightly xtask`). After it, the words stop at a redirection (`2>&1`, `> out`, `<`)
/// or a closing parenthesis, which are shell syntax, and at a placeholder. A placeholder in the
/// subcommand's place gives no command.
fn command_of(split: &[String]) -> Option<Command> {
    let mut words: Vec<String> = Vec::new();
    let mut placeholder = false;
    for raw in split.iter().map(String::as_str) {
        if words.is_empty() {
            let word = raw.trim_start_matches(['(', '$']);
            if word.is_empty() || is_prefix_word(word) {
                continue;
            }
            if word != "cargo" {
                return None;
            }
            words.push(word.to_owned());
            continue;
        }
        if words.len() == 1 && raw.len() > 1 && raw.starts_with('+') {
            continue;
        }
        if raw.starts_with('#') {
            break;
        }
        if is_placeholder(raw) {
            if words.len() == 2 {
                return None;
            }
            placeholder = true;
            break;
        }
        match raw.find(['>', '<', ')']) {
            Some(cut) => {
                let before = raw.get(..cut).unwrap_or_default().trim_end_matches('&');
                if !before.is_empty() && !before.bytes().all(|b| b.is_ascii_digit()) {
                    words.push(before.to_owned());
                }
                break;
            }
            None => words.push(raw.to_owned()),
        }
    }
    let is_command = words.first().map(String::as_str) == Some("cargo")
        && words.get(1).map(String::as_str) == Some("xtask");
    is_command.then_some(Command { words, placeholder })
}

/// True for a placeholder word: `<name>` (no other `<` or `>` in it), or a word holding `...`.
fn is_placeholder(word: &str) -> bool {
    let named = word
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .is_some_and(|name| !name.is_empty() && !name.contains(['<', '>']));
    named || word.contains("...")
}

/// A word that may come before `cargo` in a command: a prompt, an assignment, or a word that runs
/// the command (`sudo`, `time`, `env`, `exec`).
fn is_prefix_word(word: &str) -> bool {
    matches!(word, "$" | "sudo" | "time" | "env" | "exec") || is_assignment(word)
}

/// True for a shell `NAME=value` word: a name of letters, digits and `_` that does not start with
/// a digit, then `=`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The violation for `command` (as written, from `line`) when it does not parse.
fn problem(file: &str, line: u32, command: &Command) -> Option<Violation> {
    let mut argv = vec!["cargo xtask"];
    argv.extend(command.words.iter().skip(2).map(String::as_str));
    let first = parse(&argv, command.placeholder).err()?;
    let shown = argv.join(" ");
    let detail = if !first.is_empty() && first.chars().count() <= SHORT {
        format!(": {first}")
    } else {
        String::new()
    };
    Some(violation(
        file,
        line,
        format!("the xtask command `{shown}` does not parse{detail}"),
        FIX,
    ))
}

/// Parses `argv`, its first word the binary name, with the clap tree of `Cli`. `Err` holds the
/// `error:` line of clap's message, or an empty string when there is none. Asking for help parses
/// only when the command parses without the help flags, since clap stops at the first one it meets.
/// With `placeholder`, an error that only says a word is missing parses: the placeholder after
/// `argv` stands for that word.
fn parse(argv: &[&str], placeholder: bool) -> Result<(), String> {
    match Cli::command().try_get_matches_from(argv) {
        Ok(_) => Ok(()),
        Err(error) if placeholder && is_missing_word(&error) => Ok(()),
        Err(error) if is_help(&error) => {
            let rest: Vec<&str> = argv
                .iter()
                .copied()
                .filter(|word| *word != "-h" && *word != "--help")
                .collect();
            if rest.len() == 1 {
                return Ok(());
            }
            match Cli::command().try_get_matches_from(rest) {
                Err(error) if placeholder && is_missing_word(&error) => Ok(()),
                Err(error) if !is_help(&error) => Err(first_line(&error)),
                _ => Ok(()),
            }
        }
        Err(error) => Err(first_line(&error)),
    }
}

/// True for the errors that are help or version output, which clap prints and exits with.
fn is_help(error: &Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    )
}

/// True for the errors that say only that a word is missing: a subcommand, a required argument,
/// or a flag's value.
fn is_missing_word(error: &Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::MissingSubcommand
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::MissingRequiredArgument
    ) || is_missing_value(error)
}

/// True for clap's error that a flag was given without its value.
fn is_missing_value(error: &Error) -> bool {
    error.kind() == ErrorKind::InvalidValue && first_line(error).contains("a value is required")
}

/// What is wrong, from clap's message: `it names no subcommand` when the command stops before
/// one (clap then prints help, not an `error:` line), else the first line, trimmed, when it is an
/// `error:` line, else an empty string.
fn first_line(error: &Error) -> String {
    if matches!(
        error.kind(),
        ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    ) {
        return "it names no subcommand".to_owned();
    }
    let text = error.to_string();
    let line = text.lines().next().unwrap_or_default().trim();
    if line.starts_with("error:") {
        line.to_owned()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests;
