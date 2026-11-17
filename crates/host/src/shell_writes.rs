//! The files a turn's shell commands write, for the after-turn checkpoint to tell a file the
//! agent made or changed through a shell (pandoc's `-o "Reply.docx"`, `echo x > notes.md`,
//! `cp a b`, `stetdoc edit "Main Agreement.docx"`) from one the lawyer saved meanwhile: a
//! change is the agent's own only when a tool call names it or such a command writes it
//! (`clauseo_matters::Matter::after_turn_named`), whether it adds the file or modifies it.
//!
//! Only the command text in a shell call's input is read, never its output: a listing or a
//! `grep` names files it does not write. Only write targets count: `>` and `>>` redirects,
//! `-o` and `--output` values, `tee`'s files, the last argument of `cp`, and what a
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
//! A delete or a move writes no content, so [`add_input`] leaves it out (a file moved away and
//! back stays the lawyer's); [`deletes`] reads them, once for every caller: the paths a delete
//! names ([`add_deleted_input`]), a move's sources apart from where they go
//! ([`add_moved_input`]), and both sides with every write ([`add_changed_input`]).
//! Kept in memory for the turn only, never logged.

use std::collections::BTreeSet;
use std::path::Path;

use crate::stetdoc_args;

mod changes;
mod cmd;
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
/// `command` string is read in `dialect`. Input that is not JSON adds nothing. A move or a
/// delete adds nothing here: a file moved away and back (`mv a tmp && mv tmp a`) must stay the
/// lawyer's ([`add_changed_input`] adds them).
pub(crate) fn add_input(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) {
    let mut out = Out::new(targets, None);
    add_input_as(&mut out, raw_input, dialect, start, Kind::Writes);
}

/// [`add_input`] with every path a delete or a move names, sources and destinations alike
/// ([`deletes`]): what a turn's shell changed in any way. A deleted or moved path may be a
/// folder, with what is under it; `exact` gets those of the paths a caller matches only whole,
/// never as a folder: a write's target, and a move's destination named without a separator at
/// its end, which may be the new name or a folder the source goes in (`mv a.docx Archive` adds
/// `Archive`, matched whole, and `Archive/a.docx`). Every path in `exact` is in `targets` too:
/// a caller matches folders against the paths in `targets` that are not in `exact`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the no-copy guard (#1180) reads it, on top of this"
    )
)]
pub(crate) fn add_changed_input(
    targets: &mut BTreeSet<String>,
    exact: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) {
    let mut out = Out::new(targets, Some(exact));
    add_input_as(&mut out, raw_input, dialect, start, Kind::Changes);
}

/// [`add_input`] for the paths a delete program names alone ([`deletes::taken`]: `rm`,
/// `Remove-Item`, ...; no move), for the pre-write guard's folder rule. Returns whether a delete
/// in it names a target that is no path here: a word with an expansion, a glob, `~` or a
/// PowerShell array element of those, a parameter with no value, a target whose path the start
/// leaves unknown, the matter's folder itself or above it (`rm -rf .`, `rm -rf ..` run in the
/// matter's folder), a cmdlet that names no path (`Get-ChildItem | Remove-Item`), or a `cmd /c`
/// command line in one word that holds a cmd operator, `^` or `%`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the no-copy guard (#1180) reads it, on top of this"
    )
)]
pub(crate) fn add_deleted_input(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) -> bool {
    let mut out = Out::new(targets, None);
    add_input_as(&mut out, raw_input, dialect, start, Kind::Deletes);
    out.unclear
}

/// [`add_input`] for the paths a move takes away alone: the sources of `mv`, `git mv`,
/// `Move-Item`, `Rename-Item`, `cmd /c` `move` and `ren` ([`deletes::taken`]), not where they
/// go. Returns whether a move in it names a source that is no path here, as
/// [`add_deleted_input`] says for a delete.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the no-copy guard (#1180) reads it, on top of this"
    )
)]
pub(crate) fn add_moved_input(
    targets: &mut BTreeSet<String>,
    raw_input: &str,
    dialect: Dialect,
    start: Option<&str>,
) -> bool {
    let mut out = Out::new(targets, None);
    add_input_as(&mut out, raw_input, dialect, start, Kind::MovedFrom);
    out.unclear
}

/// Where a reading puts what it reads.
struct Out<'a> {
    /// Every path read.
    targets: &'a mut BTreeSet<String>,
    /// The paths matched only whole ([`add_changed_input`]), when the reading keeps them.
    exact: Option<&'a mut BTreeSet<String>>,
    /// A delete or a move names a target that is no path here ([`add_deleted_input`]).
    unclear: bool,
}

impl<'a> Out<'a> {
    fn new(targets: &'a mut BTreeSet<String>, exact: Option<&'a mut BTreeSet<String>>) -> Self {
        Self {
            targets,
            exact,
            unclear: false,
        }
    }

    /// Adds `target`, run in `cwd`; whether it is a path here ([`add`]).
    fn path(&mut self, cwd: Option<&str>, target: &str) -> bool {
        add(self.targets, cwd, target)
    }

    /// Adds `target`, run in `cwd`, as a path matched only whole: to `exact` when the reading
    /// keeps it, and to `targets` as every path is, so `exact` is a part of `targets`.
    fn whole(&mut self, cwd: Option<&str>, target: &str) {
        if self.path(cwd, target)
            && let Some(exact) = self.exact.as_deref_mut()
            && let Some(path) = place(cwd, target)
        {
            exact.insert(path);
        }
    }
}

/// Which targets a reading adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the no-copy guard (#1180) reads all but the writes"
    )
)]
enum Kind {
    /// Every write target, no move or delete.
    Writes,
    /// Every write target, and every path a delete or a move names.
    Changes,
    /// Only what a delete program names.
    Deletes,
    /// Only a move's sources.
    MovedFrom,
}

fn add_input_as(
    out: &mut Out<'_>,
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
            add_script_as(out, cwd, script, dialect, kind);
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
                    add_script_as(out, cwd, script, Dialect::Posix, kind);
                }
                _ => {
                    let words: Vec<Word> = argv.into_iter().map(Word::plain).collect();
                    let how = How {
                        kind,
                        dialect: Dialect::Posix,
                    };
                    add_segment(out, cwd, &words, &[], how);
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
    add_script_as(
        &mut Out::new(targets, None),
        cwd,
        script,
        dialect,
        Kind::Writes,
    );
}

fn add_script_as(out: &mut Out<'_>, cwd: Option<&str>, script: &str, dialect: Dialect, kind: Kind) {
    let segments = match dialect {
        Dialect::Posix => segments(script),
        Dialect::PowerShell => powershell::segments(script),
    };
    let mut folder = folder::Folder::new(cwd);
    for segment in &segments {
        if let Some((cwd, words)) = folder.read(segment, dialect) {
            let how = How { kind, dialect };
            add_segment(out, cwd.as_deref(), words, &segment.redirects, how);
        }
    }
}

/// How a simple command is read: for which targets, and in which shell.
#[derive(Debug, Clone, Copy)]
struct How {
    kind: Kind,
    dialect: Dialect,
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

/// One word of a command, whether it is plain text (no expansion, glob or `~`), and whether it
/// is a PowerShell array of plain text elements (`a.docx, 'b c.docx'`, split at
/// `ARRAY_COMMA`).
#[derive(Debug, Clone)]
struct Word {
    text: String,
    clear: bool,
    list: bool,
}

impl Word {
    fn plain(text: String) -> Self {
        Self {
            text,
            clear: true,
            list: false,
        }
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

/// Adds one simple command's targets, read as `how` says, run in `cwd` (`None`: unknown, so only
/// absolute targets count).
fn add_segment(out: &mut Out<'_>, cwd: Option<&str>, words: &[Word], redirects: &[Word], how: How) {
    // Leading assignments and wrappers that run the rest as the command.
    let start = words
        .iter()
        .position(|word| {
            !is_assignment(&word.text)
                && !["env", "command", "time", "nohup", "sudo"].contains(&word.text.as_str())
        })
        .unwrap_or(words.len());
    let argv = words.get(start..).unwrap_or_default();
    if matches!(how.kind, Kind::Writes | Kind::Changes) {
        add_writes(out, cwd, argv, redirects);
    }
    if how.kind != Kind::Writes
        && let Some((program, args)) = argv.split_first()
    {
        // `cmd /c "del a.docx"`: its command line read as cmd reads it.
        let split = cmd::split(&program.text, args);
        let args = match &split {
            cmd::Inner::Whole => args,
            cmd::Inner::Split(words) => words.as_slice(),
            cmd::Inner::Unread => {
                out.unclear = true;
                return;
            }
        };
        let taken = deletes::taken(&program.text, args, how.dialect);
        changes::add_taken(out, cwd, args, &taken, how);
    }
}

/// Adds the files one simple command's words `argv` and `redirects` write, run in `cwd`.
fn add_writes(out: &mut Out<'_>, cwd: Option<&str>, argv: &[Word], redirects: &[Word]) {
    let mut found: Vec<&Word> = redirects.iter().collect();
    if let Some((program, args)) = argv.split_first()
        && is_stetdoc(&program.text)
    {
        // stetdoc's own forms (`crate::stetdoc_args`): its default output and `--in-place`
        // write files no option names, and `--dry-run` writes none. A word that is not plain
        // text leaves what it writes unknown.
        if program.clear && args.iter().all(|arg| arg.clear) {
            let words: Vec<String> = args.iter().map(|arg| arg.text.clone()).collect();
            for target in stetdoc_args::written(&words) {
                out.whole(cwd, &target);
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
        // `--output=Reply.docx`
        for arg in args {
            if let Some(value) = arg.text.strip_prefix("--output=")
                && arg.clear
                && !value.is_empty()
            {
                out.whole(cwd, value);
            }
        }
    }
    for word in found {
        if word.clear {
            out.whole(cwd, &word.text);
        }
    }
}

/// Adds `target`, run in `cwd` (`None`: unknown), to `targets`; whether it is a path here: not
/// past [`MAX_TARGETS`], no separator at its end, and a place [`place`] gives.
fn add(targets: &mut BTreeSet<String>, cwd: Option<&str>, target: &str) -> bool {
    if targets.len() >= MAX_TARGETS || target.ends_with(['/', '\\']) {
        return false;
    }
    match place(cwd, target) {
        Some(path) => {
            targets.insert(path);
            true
        }
        None => false,
    }
}

/// `target`'s path, run in `cwd` (`None`: unknown, so an absolute target alone has one), joined
/// ([`join`]); `None` for the matter's folder itself (an empty path) or one above it.
fn place(cwd: Option<&str>, target: &str) -> Option<String> {
    let cwd = cwd.or_else(|| is_absolute(target).then_some(""))?;
    join(cwd, target).filter(|path| !path.is_empty())
}

/// `target` joined to `cwd` and normalised: separators as `/`, no `.` or empty part, each `..`
/// taking off the part before it. An absolute result keeps its root (`/`, `C:/`, `//`); a
/// relative one that climbs out of the matter's folder is `None`. A drive-relative target
/// (`C:a.docx`, `C:..\b.docx`) is joined to `cwd` when `cwd` is on that drive, else `None`.
/// A Win32 path's `\\?\` or `\\.\` prefix is read before a drive (`\\?\C:\x` is `C:/x`) or
/// `UNC\` (`\\?\UNC\server\share\x` is `//server/share/x`); any other such path is `None`.
fn join(cwd: &str, target: &str) -> Option<String> {
    let mut target = target;
    if let Some((drive, rest)) = drive_of(target).filter(|(_, rest)| !rest.starts_with(['/', '\\']))
    {
        let on_drive = drive_of(cwd).is_some_and(|(on, path)| {
            on.eq_ignore_ascii_case(&drive) && path.starts_with(['/', '\\'])
        });
        // A second drive after the first (`C:C:a.docx`) names no path.
        if !on_drive || drive_of(rest).is_some() {
            return None;
        }
        target = rest;
    }
    let whole = if is_absolute(target) || cwd.is_empty() {
        target.to_owned()
    } else {
        format!("{cwd}/{target}")
    };
    let mut unified = whole.replace('\\', "/");
    if let Some(rest) = ["//?/", "//./"]
        .iter()
        .find_map(|prefix| unified.strip_prefix(prefix))
    {
        unified = match rest.get(..4) {
            Some(unc) if unc.eq_ignore_ascii_case("UNC/") => format!("//{}", rest.get(4..)?),
            _ if drive_of(rest).is_some_and(|(_, path)| path.starts_with('/')) => rest.to_owned(),
            _ => return None,
        };
    }
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
