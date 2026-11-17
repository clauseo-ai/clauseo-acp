//! The files a turn's shell commands write, for the after-turn checkpoint to tell a file the
//! agent made or changed through a shell (pandoc's `-o "Reply.docx"`, `echo x > notes.md`,
//! `cp a b`, `stetdoc edit "Main Agreement.docx"`) from one the lawyer saved meanwhile: a
//! change is the agent's own only when a tool call names it or such a command writes it
//! (`clauseo_matters::Matter::after_turn_named`), whether it adds the file or modifies it.
//!
//! Only the command text in a shell call's input is read, never its output: a listing or a
//! `grep` names files it does not write. Only write targets count: `>` and `>>` redirects,
//! `-o` and `--output` values, `tee`'s files, the last argument of `cp`, every path a delete or a
//! move names (`rm`, `unlink`, `mv`'s both sides, `Remove-Item`, `del`: [`deletes`]), and what a
//! `stetdoc` command writes, read with its real options
//! by the parser the pre-write guard shares (`crate::stetdoc_args::written`): its output, else
//! with `--in-place` the document, else `NAME.edited.docx` beside it; nothing for `--dry-run`,
//! `view` and the other reading subcommands. The command is split as a shell does (quotes kept
//! together) into segments at `;`, `&&`, `||`, `|` and new lines, and a heredoc's body is no
//! command at all. A `cd <dir>` at any position sets the folder the segments after it run in
//! ([`folder`]: when it surely runs in this shell, else relative targets after it count for
//! nothing). Claude Code's `PowerShell` tool (an `execute` call named so) is split as
//! PowerShell does ([`powershell`]: its quotes, `\` a separator, `Set-Location`, `sl`,
//! `chdir` and `Push-Location` as `cd`). Each relative target is joined to that folder (else
//! the call's `cwd` when its input says, else the matter's folder) and normalised, so it
//! compares with a file's whole path in the matter. Each call starts there, whatever an earlier
//! call's `cd` did: Claude Code's shell goes back to the session's folder after every call,
//! since Clauseo launches it with `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`
//! (`crate::agent::loads`), and Codex's exec runs each call in its own process, from its
//! `workdir` or the turn's folder, so nothing is tracked across calls. When Claude Code's policy
//! settings may turn that variable off, the start is unknown instead (`sent::ShellStart`): only
//! absolute targets, and those after an absolute `cd`, count. A target that is unclear (an
//! expansion, a glob, one ending in `/`) writes nothing, and leaves the others as they are.
//! Kept in memory for the turn only, never logged.

use std::collections::BTreeSet;
use std::path::Path;

use crate::stetdoc_args;

pub(crate) mod deletes;
mod folder;
mod posix;
mod powershell;

use posix::segments;

/// The most targets one turn keeps.
const MAX_TARGETS: usize = 1024;

/// Commands whose `-o` is no output file (`unzip -o` overwrites, it names no output).
const NOT_OUTPUT: [&str; 11] = [
    "grep", "egrep", "fgrep", "rg", "ls", "ps", "find", "test", "[", "ssh", "unzip",
];

/// The shell a call's command is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// A POSIX shell: Claude's and Codex's `Bash`, Git Bash on Windows.
    Posix,
    /// Claude Code's `PowerShell` tool on Windows (claude-agent-acp 0.89.1 sends it as an
    /// `execute` call named `PowerShell`, its input `{"command": …}`): [`powershell`].
    PowerShell,
}

impl Dialect {
    /// The dialect of a shell call whose tool is named `tool_name` (ACP's `name`).
    pub(crate) fn of_tool(tool_name: Option<&str>) -> Self {
        if tool_name.is_some_and(|name| name.eq_ignore_ascii_case("PowerShell")) {
            Self::PowerShell
        } else {
            Self::Posix
        }
    }
}

/// Adds the targets of the command in a shell call's input (its JSON text): a `command` string,
/// or an argument list (`["bash", "-lc", "<script>"]`, or the program's own arguments), run in
/// its `cwd` or `workdir` when the input gives one, else in `start` (`""`: the matter's folder,
/// `None`: unknown, so only absolute targets and those after an absolute `cd` count). A
/// `command` string is read in `dialect`. Input that is not JSON adds nothing.
pub(crate) fn add_input(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) {
    add_input_as(targets, raw_input, dialect, start, Kind::Writes);
}

/// [`add_input`] for the paths a delete program names alone ([`deletes::deletes`]: `rm`,
/// `Remove-Item`, ...; no move), for the pre-write guard's folder rule
/// (`pre_write_guard/no_copy.rs`).
pub(crate) fn add_deleted_input(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) {
    add_input_as(targets, raw_input, dialect, start, Kind::Deletes);
}

/// Which targets a reading adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Every write target.
    Writes,
    /// Only what a delete program names.
    Deletes,
}

fn add_input_as(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
    kind: Kind,
) {
    let Ok(serde_json::Value::Object(input)) = serde_json::from_str(raw_input) else {
        return;
    };
    let cwd = ["cwd", "workdir"]
        .iter()
        .find_map(|key| input.get(*key).and_then(serde_json::Value::as_str))
        .or(start);
    match input.get("command") {
        Some(serde_json::Value::String(script)) => {
            add_script_as(targets, cwd, script, dialect, kind);
        }
        Some(serde_json::Value::Array(items)) => {
            let Some(argv) = items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()
            else {
                return;
            };
            match argv.as_slice() {
                [shell, flag, script]
                    if ["-c", "-lc", "-ic"].contains(&flag.as_str())
                        && ["sh", "bash", "zsh", "dash"].contains(&base(shell)) =>
                {
                    add_script_as(targets, cwd, script, Dialect::Posix, kind);
                }
                _ => {
                    let words: Vec<Word> = argv.into_iter().map(Word::plain).collect();
                    add_segment(targets, cwd, &words, &[], kind);
                }
            }
        }
        _ => {}
    }
}

/// Adds the targets of the shell script `script`, written in `dialect` and run in `cwd` (`""`:
/// the matter's folder, `None` unknown).
#[cfg(test)]
fn add_script(targets: &mut BTreeSet<String>, cwd: Option<&str>, script: &str, dialect: Dialect) {
    add_script_as(targets, cwd, script, dialect, Kind::Writes);
}

fn add_script_as(
    targets: &mut BTreeSet<String>,
    cwd: Option<&str>,
    script: &str,
    dialect: Dialect,
    kind: Kind,
) {
    let segments = match dialect {
        Dialect::Posix => segments(script),
        Dialect::PowerShell => powershell::segments(script),
    };
    let mut folder = folder::Folder::new(cwd);
    for segment in &segments {
        if let Some((cwd, words)) = folder.read(segment, dialect) {
            add_segment(targets, cwd.as_deref(), words, &segment.redirects, kind);
        }
    }
}

/// The path in the matter `folder` of `target` (relative to the folder, or absolute), or `None`
/// when it lies outside it.
pub(crate) fn in_matter(folder: &Path, target: &str) -> Option<String> {
    if !is_absolute(target) {
        return Some(target.to_owned());
    }
    let folder = join("", &folder.to_string_lossy())?;
    let rest = target.strip_prefix(folder.as_str())?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// One word of a command, and whether it is plain text: no expansion, glob or `~`.
#[derive(Debug)]
struct Word {
    text: String,
    clear: bool,
}

impl Word {
    fn plain(text: String) -> Self {
        Self { text, clear: true }
    }
}

impl AsRef<str> for Word {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

/// What joins a simple command to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sep {
    /// `;` or a new line.
    Then,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `|`.
    Pipe,
    /// `&` (in PowerShell a call operator too).
    Background,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// The end of the command.
    #[default]
    End,
}

/// One simple command: its words, the targets of its `>` and `>>` redirects, what joins it to
/// the next, and whether it stands inside parentheses.
#[derive(Debug, Default)]
struct Segment {
    words: Vec<Word>,
    redirects: Vec<Word>,
    ender: Sep,
    nested: bool,
}

/// Adds one simple command's targets of `kind`, run in `cwd` (`None`: unknown, so only
/// absolute targets count).
fn add_segment(
    targets: &mut BTreeSet<String>,
    cwd: Option<&str>,
    words: &[Word],
    redirects: &[Word],
    kind: Kind,
) {
    let mut found: Vec<&Word> = if kind == Kind::Writes {
        redirects.iter().collect()
    } else {
        Vec::new()
    };
    // Leading assignments and wrappers that run the rest as the command.
    let start = words
        .iter()
        .position(|word| {
            !is_assignment(&word.text)
                && !["env", "command", "time", "nohup", "sudo"].contains(&word.text.as_str())
        })
        .unwrap_or(words.len());
    let argv = words.get(start..).unwrap_or_default();
    if kind == Kind::Deletes {
        if let Some((program, args)) = argv.split_first()
            && deletes::deletes(&program.text)
        {
            // A folder deleted with a trailing separator (`rm -r Exhibits/`) is that folder.
            for word in deletes::removed(&program.text, args)
                .into_iter()
                .filter_map(|i| args.get(i))
                .filter(|word| word.clear)
            {
                let trimmed = word.text.trim_end_matches(['/', '\\']);
                add(
                    targets,
                    cwd,
                    if trimmed.is_empty() { "/." } else { trimmed },
                );
            }
        }
    } else if let Some((program, args)) = argv.split_first()
        && is_stetdoc(&program.text)
    {
        // stetdoc's own forms (`crate::stetdoc_args`): its default output and `--in-place`
        // write files no option names, and `--dry-run` writes none. A word that is not plain
        // text leaves what it writes unknown.
        if program.clear && args.iter().all(|arg| arg.clear) {
            let words: Vec<String> = args.iter().map(|arg| arg.text.clone()).collect();
            for target in stetdoc_args::written(&words) {
                add(targets, cwd, &target);
            }
        }
    } else if let Some((program, args)) = argv.split_first() {
        let program = base(&program.text);
        if !NOT_OUTPUT.contains(&program) {
            for (i, arg) in args.iter().enumerate() {
                if arg.text == "-o" || arg.text == "--output" {
                    found.extend(args.get(i + 1));
                }
            }
        }
        let values = args.iter().filter(|arg| !arg.text.starts_with('-'));
        match program {
            "tee" => found.extend(values),
            "cp" if !args.iter().any(|arg| {
                arg.text.starts_with("-t") || arg.text.starts_with("--target-directory")
            }) =>
            {
                let values: Vec<&Word> = values.collect();
                if values.len() >= 2 {
                    found.extend(values.last().copied());
                }
            }
            _ => {}
        }
        found.extend(
            deletes::removed(program, args)
                .into_iter()
                .filter_map(|i| args.get(i)),
        );
        // `--output=Reply.docx`
        for arg in args {
            if let Some(value) = arg.text.strip_prefix("--output=")
                && arg.clear
                && !value.is_empty()
            {
                add(targets, cwd, value);
            }
        }
    }
    for word in found {
        if word.clear {
            add(targets, cwd, &word.text);
        }
    }
}

fn add(targets: &mut BTreeSet<String>, cwd: Option<&str>, target: &str) {
    if targets.len() >= MAX_TARGETS || target.ends_with(['/', '\\']) {
        return;
    }
    let Some(cwd) = cwd.or_else(|| is_absolute(target).then_some("")) else {
        return;
    };
    if let Some(path) = join(cwd, target)
        && !path.is_empty()
    {
        targets.insert(path);
    }
}

/// `target` joined to `cwd` and normalised: separators as `/`, no `.` or empty part, each `..`
/// taking off the part before it. An absolute result keeps its root (`/`, `C:/`, `//`); a
/// relative one that climbs out of the matter's folder is `None`.
fn join(cwd: &str, target: &str) -> Option<String> {
    let whole = if is_absolute(target) || cwd.is_empty() {
        target.to_owned()
    } else {
        format!("{cwd}/{target}")
    };
    let unified = whole.replace('\\', "/");
    let (root, rest) = if let Some(rest) = unified.strip_prefix("//") {
        ("//".to_owned(), rest)
    } else if let Some(rest) = unified.strip_prefix('/') {
        ("/".to_owned(), rest)
    } else if let Some((drive, rest)) = drive_of(&unified) {
        (format!("{drive}:"), rest.trim_start_matches('/'))
    } else {
        (String::new(), unified.as_str())
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() && root.is_empty() {
                    return None;
                }
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    Some(match root.as_str() {
        "" => joined,
        "/" | "//" => format!("{root}{joined}"),
        drive => format!("{drive}/{joined}"),
    })
}

/// A Windows path's drive letter and what follows its colon: `C:\Matter` is `('C', "\Matter")`.
fn drive_of(path: &str) -> Option<(char, &str)> {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => Some((letter, chars.as_str())),
        _ => None,
    }
}

fn is_absolute(path: &str) -> bool {
    path.starts_with(['/', '\\']) || drive_of(path).is_some()
}

/// Whether `program` is stetdoc: its name without its folder, in any case, `.exe` or not.
fn is_stetdoc(program: &str) -> bool {
    let name = base(program).to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name) == "stetdoc"
}

/// The program's name without its folder.
fn base(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

/// `NAME=value`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_before;
