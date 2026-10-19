//! `cargo xtask` commands in the docs. Every code span and every line of a code block must parse,
//! wherever `cargo xtask` starts a command in it: after a `$` prompt, after `VAR=value`
//! assignments, or after `&&`, `||`, `;` or `|`. Each parses against the clap tree in `main.rs`.

use super::markdown::{Line, Scan};
use crate::Cli;
use crate::checks::violation;
use crate::violation::Violation;
use clap::CommandFactory;
use clap::error::{Error, ErrorKind};

/// The longest clap first line that the message repeats. A longer one is left out.
const SHORT: usize = 80;

/// The one fix for every command that does not parse.
const FIX: &str = "correct it to a command that `cargo xtask --help` lists (`cargo xtask \
    <command> --help` lists its flags), or remove it";

/// One violation per xtask command in `file` that does not parse.
pub(super) fn problems(file: &str, scan: &Scan) -> Vec<Violation> {
    let spans = scan.spans.iter().map(|span| (span.line, span.text.clone()));
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

/// The xtask commands in `text`, each as its words joined by single spaces, starting `cargo xtask`.
/// The text is cut at `&&`, `||`, `;` and `|`; each part may begin with a `$` prompt and
/// `NAME=value` assignments.
fn commands_in(text: &str) -> Vec<String> {
    text.replace("&&", ";")
        .replace("||", ";")
        .replace('|', ";")
        .split(';')
        .filter_map(command_of)
        .collect()
}

/// The command a part of a line holds, when it holds one (see `commands_in`).
fn command_of(part: &str) -> Option<String> {
    let words: Vec<&str> = part
        .split_whitespace()
        .skip_while(|word| *word == "$" || is_assignment(word))
        .take_while(|word| !word.starts_with('#'))
        .collect();
    (words.first() == Some(&"cargo") && words.get(1) == Some(&"xtask")).then(|| words.join(" "))
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
fn problem(file: &str, line: u32, command: &str) -> Option<Violation> {
    let mut argv = vec!["cargo xtask"];
    argv.extend(command.split_whitespace().skip(2));
    let first = parse(&argv).err()?;
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
fn parse(argv: &[&str]) -> Result<(), String> {
    match Cli::command().try_get_matches_from(argv) {
        Ok(_) => Ok(()),
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

/// The first line of clap's message, trimmed, when it is an `error:` line. Clap's other messages
/// (the help it prints for a command with no subcommand) say nothing about the error, so they
/// give an empty string.
fn first_line(error: &Error) -> String {
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
