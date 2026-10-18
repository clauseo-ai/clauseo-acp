//! `cargo xtask` commands in the docs. Every code span and every fenced line that starts with
//! `cargo xtask` must parse against the clap tree in `main.rs`, as the binary would read it.

use super::markdown::{Line, Scan, Span};
use crate::Cli;
use crate::checks::violation;
use crate::violation::Violation;
use clap::CommandFactory;
use clap::error::ErrorKind;

/// The longest clap first line that the message repeats. A longer one is left out.
const SHORT: usize = 80;

/// The one fix for every command that does not parse.
const FIX: &str = "correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it";

/// One violation per xtask command in `file` that does not parse.
pub(super) fn problems(file: &str, scan: &Scan) -> Vec<Violation> {
    let spans = scan
        .spans
        .iter()
        .filter(|span| starts_with_xtask(&span.text))
        .map(|span: &Span| (span.line, span.text.clone()));
    let fenced = fenced_commands(&scan.fenced).into_iter();
    spans
        .chain(fenced)
        .filter_map(|(line, command)| problem(file, line, &command))
        .collect()
}

/// True when `text` starts with the two words `cargo xtask`.
fn starts_with_xtask(text: &str) -> bool {
    let mut words = text.split_whitespace();
    words.next() == Some("cargo") && words.next() == Some("xtask")
}

/// The commands in fenced lines: each line that starts with `cargo xtask`, with the lines that
/// continue it after a trailing backslash joined to it. Each is paired with its first line.
fn fenced_commands(lines: &[Line]) -> Vec<(u32, String)> {
    let mut commands = Vec::new();
    let mut open: Option<(u32, String)> = None;
    for line in lines {
        let text = line.text.trim();
        let (body, continues) = text
            .strip_suffix('\\')
            .map_or((text, false), |body| (body.trim_end(), true));
        let command = match open.take() {
            Some((start, mut command)) => {
                command.push(' ');
                command.push_str(body);
                (start, command)
            }
            None if starts_with_xtask(body) => (line.line, body.to_owned()),
            None => continue,
        };
        if continues {
            open = Some(command);
        } else {
            commands.push(command);
        }
    }
    commands.extend(open);
    commands
}

/// The violation for `command` (as written, from `line`) when it does not parse.
fn problem(file: &str, line: u32, command: &str) -> Option<Violation> {
    let mut argv = vec!["cargo xtask"];
    argv.extend(
        command
            .split_whitespace()
            .skip(2)
            .take_while(|word| !word.starts_with('#')),
    );
    let first = match parse(&argv) {
        Ok(()) => return None,
        Err(first) => first,
    };
    let shown = argv.join(" ");
    let detail = if first.chars().count() <= SHORT {
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

/// Parses `argv`, the first word the binary name, with the clap tree of `Cli`. `Err` holds the
/// first line of clap's error. Asking for help or the version parses.
fn parse(argv: &[&str]) -> Result<(), String> {
    match Cli::command().try_get_matches_from(argv) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()),
    }
}

#[cfg(test)]
mod tests;
