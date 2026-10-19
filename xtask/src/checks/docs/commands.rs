//! `cargo xtask` commands in the docs. Every code span and every line of a code block must parse,
//! wherever `cargo xtask` starts a command in it: after a `$` prompt, after `VAR=value`
//! assignments, or after `&&`, `||`, `;` or `|`. Each parses against the clap tree in `main.rs`.
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

/// The span that names the tool in prose, not a command to run.
const TOOL: &str = "cargo xtask";

/// An xtask command found in the docs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Command {
    /// Its words joined by single spaces, starting `cargo xtask`, up to the first placeholder.
    text: String,
    /// True when a placeholder followed these words.
    placeholder: bool,
}

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
                .filter_map(move |command| problem(file, line, &command))
        })
        .collect()
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

/// The xtask commands in `text`. The text is cut at `&&`, `||`, `;` and `|`; each part may begin
/// with a `$` prompt and `NAME=value` assignments.
fn commands_in(text: &str) -> Vec<Command> {
    text.replace("&&", ";")
        .replace("||", ";")
        .replace('|', ";")
        .split(';')
        .filter_map(command_of)
        .collect()
}

/// The command a part of a line holds, when it holds one (see `commands_in`). Before `cargo`, a
/// `(` or `$(`, a `time`, `env` or `exec`, and assignments are skipped. After it, the words stop
/// at a redirection (`2>&1`, `> out`, `<`) or a closing parenthesis, which are shell syntax, and
/// at a placeholder. A placeholder in the subcommand's place gives no command.
fn command_of(part: &str) -> Option<Command> {
    let mut words: Vec<&str> = Vec::new();
    let mut placeholder = false;
    for raw in part.split_whitespace() {
        if words.is_empty() {
            let word = raw.trim_start_matches(['(', '$']);
            if word.is_empty() || is_prefix_word(word) {
                continue;
            }
            if word != "cargo" {
                return None;
            }
            words.push(word);
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
                    words.push(before);
                }
                break;
            }
            None => words.push(raw),
        }
    }
    let is_command = words.first() == Some(&"cargo") && words.get(1) == Some(&"xtask");
    is_command.then(|| Command {
        text: words.join(" "),
        placeholder,
    })
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
/// the command (`time`, `env`, `exec`).
fn is_prefix_word(word: &str) -> bool {
    matches!(word, "$" | "time" | "env" | "exec") || is_assignment(word)
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
    argv.extend(command.text.split_whitespace().skip(2));
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
