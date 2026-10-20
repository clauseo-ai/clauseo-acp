//! `cargo xtask` commands in the docs. An inline code span is read whole. A code block is read
//! line by line when its info word is in `SHELLS`, when it has no info string, or when it is
//! indented. A session block is read only on its lines that start with a prompt from `PROMPTS`,
//! and only from after that prompt. Other code blocks are not read. Each read line is split into
//! words with `shlex`, once, and a line that `shlex` cannot split is not read. Every `cargo` in the
//! words, with its flags and `+toolchain` skipped, followed by `xtask`, starts a command. The words
//! after `xtask` stop at shell syntax and parse against the clap tree in `main.rs`.

use super::markdown::Scan;
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

/// What ends a word as shell syntax: a redirection (`2>&1`, `> out`, `<`), a closing parenthesis
/// or backtick, and an operator glued to the word (`check;`, `check&`).
const SHELL_PUNCTUATION: [char; 7] = ['>', '<', ')', '`', ';', '&', '|'];

/// The info words of a code block whose every line is a shell line.
const SHELLS: [&str; 10] = [
    "sh",
    "bash",
    "zsh",
    "fish",
    "shell",
    "powershell",
    "pwsh",
    "ps1",
    "cmd",
    "bat",
];

/// The info words of a code block that shows a shell session: prompted commands and their output.
const SESSIONS: [&str; 6] = [
    "console",
    "terminal",
    "shell-session",
    "sh-session",
    "pwsh-session",
    "ps1-session",
];

/// The prompts a session line starts with. `# ` and `> ` are not prompts: in a session they start
/// a comment or a quoted output line as often.
const PROMPTS: [&str; 3] = ["$ ", "% ", "PS> "];

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

/// One violation per xtask command in `file` that does not parse.
pub(super) fn problems(file: &str, scan: &Scan) -> Vec<Violation> {
    let spans = scan
        .spans
        .iter()
        .filter(|span| span.text != TOOL)
        .map(|span| (span.line, span.text.as_str()));
    let code = scan
        .code
        .iter()
        .filter_map(|line| Some((line.line, read_line(&line.info, &line.text)?)));
    spans
        .chain(code)
        .flat_map(|(line, text)| {
            commands_in(text)
                .into_iter()
                .filter_map(move |command| problem(file, line, &command))
        })
        .collect()
}

/// The text of a line of a code block whose info word is `info` (empty for a block with no info
/// string, or an indented block), or `None` when the rule does not read the line.
fn read_line<'a>(info: &str, line: &'a str) -> Option<&'a str> {
    if info.is_empty() || is_one_of(info, &SHELLS) {
        Some(line)
    } else if is_one_of(info, &SESSIONS) {
        after_prompt(line)
    } else {
        None
    }
}

/// True when `info` is one of `words`, in any case.
fn is_one_of(info: &str, words: &[&str]) -> bool {
    words.iter().any(|word| info.eq_ignore_ascii_case(word))
}

/// What follows the prompt of a session line, or `None` for a line with no prompt (output).
fn after_prompt(text: &str) -> Option<&str> {
    let text = text.trim_start();
    PROMPTS.iter().find_map(|prompt| text.strip_prefix(prompt))
}

/// The xtask commands in `line`, split into words as a shell splits them. A line `shlex` cannot
/// split has none.
fn commands_in(line: &str) -> Vec<Command> {
    let Some(words) = shlex::split(line) else {
        return Vec::new();
    };
    (0..words.len())
        .filter_map(|at| command_at(&words, at))
        .collect()
}

/// The command that starts at word `at` of `words`: `cargo`, then any `+toolchain` and flags
/// (words that start with `-`), then `xtask`, then the words up to shell syntax or a placeholder.
/// `None` when no command starts there.
fn command_at(words: &[String], at: usize) -> Option<Command> {
    let word = |index: usize| words.get(index).map(String::as_str);
    if !word(at).is_some_and(is_cargo) {
        return None;
    }
    let mut xtask = at + 1;
    while word(xtask).is_some_and(|w| w.starts_with('-') || is_toolchain(w)) {
        xtask += 1;
    }
    let (name, ended) = unpunctuated(word(xtask)?);
    if name != "xtask" {
        return None;
    }
    // `$(cargo xtask)`: the punctuation on `xtask` ends the command there.
    let after: &[String] = if ended {
        &[]
    } else {
        words.get(xtask + 1..).unwrap_or_default()
    };
    let mut command = Command {
        words: vec!["cargo".to_owned(), "xtask".to_owned()],
        placeholder: false,
    };
    for raw in after.iter().map(String::as_str) {
        if is_placeholder(raw) {
            if command.words.len() == 2 {
                return None;
            }
            command.placeholder = true;
            break;
        }
        let (head, cut) = unpunctuated(raw);
        // The file descriptor of a redirection (`2` in `2>&1`) is not an argument.
        let descriptor = cut && head.bytes().all(|b| b.is_ascii_digit());
        if !head.is_empty() && !descriptor {
            command.words.push(head.to_owned());
        }
        if cut {
            break;
        }
    }
    Some(command)
}

/// True for a word that selects a toolchain, `+nightly`.
fn is_toolchain(word: &str) -> bool {
    word.len() > 1 && word.starts_with('+')
}

/// True for a word that runs cargo: `cargo`, or a word that ends in `cargo` right after a `(`
/// (`(cargo`, `$(cargo`, `x=$(cargo`) or a backtick.
fn is_cargo(word: &str) -> bool {
    word == "cargo" || ["(cargo", "`cargo"].iter().any(|end| word.ends_with(end))
}

/// `word` up to its first `SHELL_PUNCTUATION`, and true when something was cut.
fn unpunctuated(word: &str) -> (&str, bool) {
    match word.find(SHELL_PUNCTUATION) {
        Some(cut) => (word.get(..cut).unwrap_or_default(), true),
        None => (word, false),
    }
}

/// True for a placeholder word: `<name>` (no other `<` or `>` in it), or a word holding `...`.
fn is_placeholder(word: &str) -> bool {
    let named = word
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .is_some_and(|name| !name.is_empty() && !name.contains(['<', '>']));
    named || word.contains("...")
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
