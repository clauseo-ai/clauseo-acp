//! `cargo xtask` commands in the docs. Every code span and every line of a code block must parse,
//! wherever `cargo [+toolchain] xtask` appears in it. The text is cut at `&&`, `||`, `;` and `|`,
//! and each part is split into words with `shlex`, as a shell splits it, so quotes are read
//! (`cargo xtask "check"`, `FOO="a b" cargo xtask check`). The command is found anywhere in the
//! words, so what comes before it (a prompt such as `$`, `%` or `PS>`, `sudo -E`, `env -i`,
//! `nohup`, `x=$(`) needs no list. The words after `xtask` stop at shell syntax and parse against
//! the clap tree in `main.rs`. A part that holds `cargo xtask` but that `shlex` cannot split (a
//! quote left open) fails.
//!
//! What is read, by where the text is:
//! - an inline code span: the whole span, `cargo xtask` found anywhere in its words;
//! - a code block with a shell info word (`SHELLS`): every line, found anywhere;
//! - a code block with no info string, or an indented one: a line that starts (after an optional
//!   `$ ` or `% ` prompt) with `cargo [+toolchain] xtask`, since such a block holds output and
//!   prose as often as commands;
//! - a session block (`SESSIONS`): a line that starts with a prompt (`PROMPTS`, or
//!   `PS <path>> `), from after the prompt; the other lines are output. A prompted line that ends
//!   in `\` continues onto the next, with that line's `> ` continuation prompt cut.
//!
//! A block with any other info string (`rust`, `yaml`, `md`, `text`, ...) holds code or prose
//! in which `cargo xtask` is a mention, and is not read. The info string's first word, cut at `,`
//! or whitespace, is matched in any case.

use super::markdown::{CodeLine, Scan};
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

/// The fix for a command line that `shlex` cannot split into words.
const UNSPLIT_FIX: &str = "close the quote or escape that is left open, or remove the command";

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

/// The prompts a session line starts with. `PS <path>> ` is read apart (see `after_prompt`).
/// `# ` and `> ` are not prompts: in a session they start a comment or a quoted output line as
/// often.
const PROMPTS: [&str; 3] = ["$ ", "% ", "PS> "];

/// The prompts a line of a block with no info string may start with.
const PLAIN_PROMPTS: [&str; 2] = ["$ ", "% "];

/// The prompt a continued session line starts with, cut before the lines are joined.
const CONTINUATION: &str = "> ";

/// How the command rule reads a code block, by its info word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// A shell block: every line, `cargo xtask` found anywhere.
    Shell,
    /// A session block: prompted lines only, from after the prompt.
    Session,
    /// No info string: lines that start with `cargo [+toolchain] xtask`, after a prompt.
    Plain,
    /// Any other info string: not read.
    Skip,
}

impl Reading {
    /// The reading of a block whose info word is `info` (empty when it has none).
    fn of(info: &str) -> Reading {
        if info.is_empty() {
            Reading::Plain
        } else if is_one_of(info, &SHELLS) {
            Reading::Shell
        } else if is_one_of(info, &SESSIONS) {
            Reading::Session
        } else {
            Reading::Skip
        }
    }

    /// The text of a logical line this reading reads, or `None` when it does not read it.
    fn text(self, line: &str) -> Option<&str> {
        match self {
            Reading::Shell => Some(line),
            Reading::Session => after_prompt(line),
            Reading::Plain => {
                let line = line.trim_start();
                let rest = PLAIN_PROMPTS
                    .iter()
                    .find_map(|prompt| line.strip_prefix(prompt))
                    .unwrap_or(line);
                let words: Vec<&str> = rest.split_whitespace().take(3).collect();
                (words.first() == Some(&"cargo") && xtask_at(&words).is_some()).then_some(rest)
            }
            Reading::Skip => None,
        }
    }
}

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
    let code = scan
        .code
        .chunk_by(|a, b| a.block == b.block)
        .flat_map(|block| {
            let reading = Reading::of(block.first().map_or("", |line| line.info.as_str()));
            let read = if reading == Reading::Skip { &[] } else { block };
            logical_lines(read, reading == Reading::Session)
                .into_iter()
                .filter_map(move |(line, text)| Some((line, reading.text(&text)?.to_owned())))
        });
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

/// True when `info` is one of `words`, in any case.
fn is_one_of(info: &str, words: &[&str]) -> bool {
    words.iter().any(|word| info.eq_ignore_ascii_case(word))
}

/// What follows the prompt of a session line, or `None` for a line with no prompt (output).
fn after_prompt(text: &str) -> Option<&str> {
    let text = text.trim_start();
    PROMPTS
        .iter()
        .find_map(|prompt| text.strip_prefix(prompt))
        .or_else(|| {
            text.strip_prefix("PS ")?
                .split_once("> ")
                .map(|(_, rest)| rest)
        })
}

/// The lines of one code block as logical lines: a line ending in a backslash continues onto the
/// next, and the pair is one line, with the first line's number. With `session`, a continued
/// line's `> ` prompt is cut first.
fn logical_lines(lines: &[CodeLine], session: bool) -> Vec<(u32, String)> {
    let mut logical = Vec::new();
    let mut open: Option<(u32, String)> = None;
    for line in lines {
        let mut text = line.text.trim();
        if session && open.is_some() {
            text = text.strip_prefix(CONTINUATION).unwrap_or(text).trim_start();
        }
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
/// `shlex`.
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

/// True when the words of `part`, cut at whitespace, hold `cargo [+toolchain] xtask`.
fn names_the_tool(part: &str) -> bool {
    let words: Vec<&str> = part.split_whitespace().collect();
    xtask_at(&words).is_some()
}

/// The index of the `xtask` word of the first `cargo [+toolchain] xtask` in `words`. The
/// `xtask` word may carry shell punctuation (`$(cargo xtask)`).
fn xtask_at<S: AsRef<str>>(words: &[S]) -> Option<usize> {
    let word = |at: usize| words.get(at).map(AsRef::as_ref);
    (0..words.len()).find_map(|at| {
        if !word(at).is_some_and(is_cargo) {
            return None;
        }
        let toolchain = word(at + 1).is_some_and(|w| w.len() > 1 && w.starts_with('+'));
        let next = if toolchain { at + 2 } else { at + 1 };
        word(next)
            .is_some_and(|w| unpunctuated(w).0 == "xtask")
            .then_some(next)
    })
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

/// The command `split`, a part of a line split by `shlex`, holds, when it holds one (see
/// `xtask_at`). The words after `xtask` stop at a word with `SHELL_PUNCTUATION` (the text before
/// it is kept unless it is a redirection's file descriptor, `2` in `2>&1`), and at a placeholder.
/// A placeholder in the subcommand's place gives no command.
fn command_of(split: &[String]) -> Option<Command> {
    let at = xtask_at(split)?;
    let mut words = vec!["cargo".to_owned(), "xtask".to_owned()];
    let mut placeholder = false;
    // `$(cargo xtask)`: the punctuation on `xtask` ends the command there.
    let ended = split.get(at).is_some_and(|word| unpunctuated(word).1);
    let after = if ended {
        &[]
    } else {
        split.get(at + 1..).unwrap_or_default()
    };
    for raw in after.iter().map(String::as_str) {
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
        let (head, cut) = unpunctuated(raw);
        let descriptor = cut && head.bytes().all(|b| b.is_ascii_digit());
        if !head.is_empty() && !descriptor {
            words.push(head.to_owned());
        }
        if cut {
            break;
        }
    }
    Some(Command { words, placeholder })
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
