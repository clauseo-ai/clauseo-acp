//! The pre-write guard: the command an agent's `PreToolUse` hook runs before each file edit and
//! shell command, refusing a write to a document of the matter that Word (or, on Windows, any
//! program) holds open, in every permission mode.
//!
//! [`run`] reads one hook input object (Claude Code's or Codex's) and finds the files the tool
//! would write:
//!
//! - Claude Code's `Edit`, `Write`, `MultiEdit` and `NotebookEdit`: `tool_input.file_path` or
//!   `tool_input.notebook_path`.
//! - Codex's `apply_patch` (hooked under that name or its aliases `Write` and `Edit`): the
//!   `*** Add File:`, `*** Update File:`, `*** Delete File:` and `*** Move to:` lines of the
//!   patch in `tool_input.command`.
//! - `Bash` (both agents) and Claude Code's `PowerShell`: a heuristic. `tool_input.command` is
//!   split into words the simple shell way (whitespace, quotes, and the escape character: a
//!   backslash, or PowerShell's backtick; no expansion), grouped into simple commands at each
//!   unquoted newline, `;`, `&&`, `|` and `||`; the first [`MAX_WORDS`] words are kept, and a
//!   word names a target when it is an existing file, also once its shell punctuation is off
//!   (`words.rs`, `candidates`). A word is taken from the input's `cwd` and from the folder
//!   the command's own `cd`, `pushd` or `Set-Location` moved to by then, followed by its text
//!   (`shell.rs`); a `cd` it cannot follow (`cd "$(…)"`) also takes each later relative word
//!   from `cwd` with its leading `..` parts off. PowerShell's `\` separates folders on every OS.
//!   The patch lines in the command count too, from `cwd`: Codex's `exec_command` applies
//!   `apply_patch <<'EOF' …` as a patch. A command that reaches a file another way (a glob, a
//!   variable, a script, a .NET call such as `[IO.File]::WriteAllText('Contract.docx', …)`) is
//!   not caught.
//!
//! The matter folder is the `--matter` argument when the hook passes one (Claude Code's entry
//! does, `agent/loads.rs`: Claude Code's `cwd` follows the agent's `cd`), else the input's
//! `cwd` (Codex's hook: its `cwd` is the turn's working folder, which a command's `cd` does not
//! move). A relative target is taken from the input's `cwd`. Each target is checked twice with
//! [`clauseo_matters::held_open_at`] against the matter folder, the permission check's own rule
//! (both sides resolved canonically: symlinks, `..`, case on macOS and Windows; a target outside
//! the folder not checked): as spelled, and normalised by its text first (`a/../b` is `b`, `.`
//! dropped) with its deepest existing folder then canonicalised, so a `..` after a folder that
//! does not exist yet (`Drafts/../Contract.docx`, which `mkdir -p Drafts` would make good) is
//! caught. The first one held open is refused.
//!
//! One more rule for Codex's writes: a patch target in the matter's `.codex` folder (the folder
//! or anything under it; a move's source and destination both) is refused, held open or not.
//! codex-acp trusts the session folder, so a `.codex/config.toml` there loads, and in
//! workspace-write Codex could write one without asking (clauseo-acp-private#184). A target
//! counts when either resolution puts it there: canonical (its deepest existing folder
//! canonicalised, the rest added, then [`clauseo_matters::matter_path_in`], so a symlinked alias
//! of the matter, another case or a `\\?\` prefix is caught even before `.codex` exists) or
//! lexical (`..` takes a part off, so `sub/../.codex/x` is caught with no `sub` yet). The name
//! compares ignoring ASCII case and trailing dots and spaces, as macOS and Windows file systems
//! do. In a shell command, where the working folder is unknown, any patch target with a `.codex`
//! part is refused, and under Codex's hook (the one with no `--matter` argument) so is a `Bash`
//! command any of whose words, once its shell punctuation is off, has a `.codex` part, relative
//! or absolute: `touch .codex/config.toml`, `echo x > .codex/config.toml`, and reads such as
//! `cat .codex/config.toml` too, since an agent has no reason to touch a folder Clauseo refuses
//! it. Claude Code needs no such rule: its `settingSources` leave out project settings.
//!
//! A refusal is exit code 2 with the reason on stderr, which Claude Code and Codex both take as a
//! block; stdout stays empty, so a login profile's output under Codex's `$SHELL -lc` cannot spoil
//! it. An unknown tool, or a known one with nothing held, is exit 0 with nothing written.
//!
//! It fails open, a decision (clauseo-acp-private#162): input it cannot read (empty, cut short,
//! not JSON), or a shape it does not know (no absolute `cwd` or `--matter`, no `tool_name` or
//! `tool_input`, a checked tool without its path field), is exit 0 with one stderr line starting
//! [`IGNORED`] saying why, naming no input content. Stdin comes from the agent CLI, not the
//! model, so a model cannot make it malformed; and failing closed would refuse every matched
//! tool, stopping the agent outright, the day Claude Code or Codex changes its input shape. The
//! current shapes are pinned by fixtures (`tests/fixtures/pre_write_guard`), so such a change
//! fails CI instead of opening the guard silently. In default and plan mode
//! the permission check (ADR 0021) still stops a write to a held file; in acceptEdits and
//! bypassPermissions this guard is the only stop. It never panics.

mod shell;
mod words;

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterPath;
use serde_json::Value;

use shell::{resolutions, shell_targets};
use words::Escape;

/// The first argument that makes the desktop binary run [`run`] on its stdin and stderr and
/// exit, before anything else starts. The hook entry passes it (`agent/loads.rs`).
pub const FLAG: &str = "--pre-write-guard";

/// The argument after [`FLAG`] that names the matter folder, followed by the folder.
pub const MATTER_FLAG: &str = "--matter";

/// How many of a shell command's words are looked at.
pub const MAX_WORDS: usize = 64;

/// The exit code that refuses the tool; the reason goes to stderr.
pub const DENY: i32 = 2;

/// The most input read: a hook object is a few kilobytes, a patch rarely more than a megabyte.
/// Anything longer does not parse and fails open.
const MAX_INPUT: u64 = 16 * 1024 * 1024;

/// The guard. `args` are the arguments after [`FLAG`] (`--matter <folder>`, or none); one hook
/// input object comes from `input`. Returns the exit code: [`DENY`], with the reason and a newline
/// on `errors`, when a target is refused; 0 with an [`IGNORED`] line when the input could not be
/// read; else 0 with nothing written.
pub fn run(args: &[OsString], input: impl Read, mut errors: impl Write) -> i32 {
    let mut bytes = Vec::new();
    let found = match input.take(MAX_INPUT).read_to_end(&mut bytes) {
        Ok(_) => refusal(&bytes, matter_arg(args).as_deref()),
        Err(_) => Err(Ignored::Unreadable),
    };
    let (line, code) = match found {
        Ok(None) => return 0,
        Ok(Some(refusal)) => (refusal.reason(), DENY),
        Err(ignored) => (ignored.line(), 0),
    };
    // Nothing more can be done when the agent stopped reading; the exit code still decides.
    drop(
        errors
            .write_all(line.as_bytes())
            .and_then(|()| errors.write_all(b"\n"))
            .and_then(|()| errors.flush()),
    );
    code
}

/// The start of the stderr line saying the guard ignored its input ([`Ignored::line`]).
pub const IGNORED: &str = "Clauseo's pre-write guard ignored its input";

/// Why the guard let a tool go without reading what it writes: input it could not read, or a
/// shape it does not know. The tool goes on (fail open: see the module doc), with one stderr
/// line saying why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ignored {
    /// Reading stdin failed.
    Unreadable,
    /// Stdin was empty or only whitespace.
    Empty,
    /// Stdin was not JSON, or was cut short.
    NotJson,
    /// No absolute `cwd` and no `--matter` folder.
    NoFolder,
    /// No `tool_name` string or no `tool_input`.
    NoTool,
    /// A known tool's `tool_input` lacked the field it names.
    Shape(&'static str),
}

impl Ignored {
    /// The stderr line, without its newline. It names no input content.
    fn line(self) -> String {
        let why = match self {
            Self::Unreadable => "stdin could not be read".to_owned(),
            Self::Empty => "stdin was empty".to_owned(),
            Self::NotJson => "stdin was not JSON, or was cut short".to_owned(),
            Self::NoFolder => "it has no absolute cwd and no --matter was given".to_owned(),
            Self::NoTool => "it has no tool_name or tool_input".to_owned(),
            Self::Shape(wanted) => format!("its tool_input has no {wanted}"),
        };
        format!("{IGNORED}: {why}. The tool goes on unchecked.")
    }
}

/// The folder after [`MATTER_FLAG`], when `args` start with it and it is absolute.
fn matter_arg(args: &[OsString]) -> Option<PathBuf> {
    match args {
        [flag, folder, ..] if flag == MATTER_FLAG => {
            Some(PathBuf::from(folder)).filter(|folder| folder.is_absolute())
        }
        _ => None,
    }
}

/// Why the guard refuses a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    /// A target is held open by Word or another program.
    HeldOpen(MatterPath),
    /// A Codex patch writes in the matter's `.codex` folder, or a Codex shell command names a
    /// `.codex` folder.
    CodexFolder,
}

impl Refusal {
    /// What the agent is told.
    fn reason(&self) -> String {
        match self {
            Self::HeldOpen(path) => format!(
                "{path} is open in Word or another program on this computer. A change made now \
                 would be lost when that program saves it. Ask the lawyer to close it, or write \
                 to a copy with another name."
            ),
            Self::CodexFolder => "Clauseo doesn't let the agent change the .codex folder in a \
                                  matter: files there change what the agent loads and runs."
                .to_owned(),
        }
    }
}

/// Why the hook input `bytes` is refused, if it is, with `matter` the folder from the
/// arguments: a patch target in the matter's `.codex` folder, or under Codex's hook (no
/// `matter`) a `Bash` word with a `.codex` part, else the first target that lies in the matter
/// folder and is held open. [`Ignored`] when the input cannot be read or has a shape the guard
/// does not know.
fn refusal(bytes: &[u8], matter: Option<&Path>) -> Result<Option<Refusal>, Ignored> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(Ignored::Empty);
    }
    let hook: Value = serde_json::from_slice(bytes).map_err(|_not_json| Ignored::NotJson)?;
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute());
    let folder = matter
        .map(Path::to_path_buf)
        .or_else(|| cwd.clone())
        .ok_or(Ignored::NoFolder)?;
    let base = cwd.unwrap_or_else(|| folder.clone());
    let tool = hook
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or(Ignored::NoTool)?;
    let input = hook.get("tool_input").ok_or(Ignored::NoTool)?;
    let Some(targets) = targets(tool, input, &base)? else {
        return Ok(None);
    };
    let into_codex = match &targets {
        Targets::Named(_) => false,
        Targets::Patch(paths) => paths
            .iter()
            .any(|path| in_codex_folder(&folder, &base, path)),
        Targets::Shell {
            patch, names_codex, ..
        } => {
            patch.iter().any(|path| has_codex_part(path))
                || (*names_codex && matter.is_none() && tool == "Bash")
        }
    };
    if into_codex {
        return Ok(Some(Refusal::CodexFolder));
    }
    Ok(targets
        .paths()
        .flat_map(|path| resolutions(&base.join(path)))
        .find_map(|path| clauseo_matters::held_open_at(&folder, &path))
        .map(|(path, _)| Refusal::HeldOpen(path)))
}

/// The paths a tool would write, as the agent spelled them, and where they came from.
enum Targets {
    /// A file tool's path.
    Named(Vec<PathBuf>),
    /// The files of a Codex `apply_patch` patch.
    Patch(Vec<PathBuf>),
    /// A shell command: the existing files it names, the files of a patch in its text, and
    /// whether a word of it has a `.codex` part.
    Shell {
        files: Vec<PathBuf>,
        patch: Vec<PathBuf>,
        names_codex: bool,
    },
}

impl Targets {
    /// Every path to check for a file held open.
    fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        let (first, second): (&[PathBuf], &[PathBuf]) = match self {
            Self::Named(paths) | Self::Patch(paths) => (paths, &[]),
            Self::Shell { files, patch, .. } => (files, patch),
        };
        first.iter().chain(second)
    }
}

/// The paths the tool `tool` with `input` would write, `None` for a tool the guard does not
/// check; a shell command's words are files when they exist, taken from `base`. A checked tool
/// whose input lacks the field its paths come from is ignored.
fn targets(tool: &str, input: &Value, base: &Path) -> Result<Option<Targets>, Ignored> {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let targets = match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
            if let Some(path) = text("file_path").or_else(|| text("notebook_path")) {
                Targets::Named(vec![PathBuf::from(path)])
            } else if let Some(patch) = text("command") {
                Targets::Patch(patch_targets(patch))
            } else {
                return Err(Ignored::Shape("file_path, notebook_path or command string"));
            }
        }
        "Bash" => shell_targets(input.get("command"), base, Escape::Backslash)?,
        "PowerShell" => shell_targets(input.get("command"), base, Escape::Backtick)?,
        _ => return Ok(None),
    };
    Ok(Some(targets))
}

/// Whether `path`, taken from `base`, is `<folder>/.codex` or lies under it, resolved
/// canonically or lexically.
fn in_codex_folder(folder: &Path, base: &Path, path: &Path) -> bool {
    let absolute = base.join(path);
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    if let Some(inside) = clauseo_matters::matter_path_in(&root, &existing_resolved(&absolute))
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

/// `path` with its deepest existing folder canonicalised and the rest added lexically.
fn existing_resolved(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(found) = dunce::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return lexical(&found.join(rest));
        }
    }
    lexical(path)
}

/// `path` with `.` parts dropped and each `..` taking the part before it off.
fn lexical(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => resolved.push(part),
        }
    }
    resolved
}

/// Whether `name` is `.codex`, ignoring ASCII case and trailing dots and spaces.
fn is_codex(name: &str) -> bool {
    name.trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(".codex")
}

/// Whether any part of `path`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
fn has_codex_part(path: &Path) -> bool {
    path.to_str().is_some_and(text_has_codex_part)
}

/// Whether any part of `text`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
fn text_has_codex_part(text: &str) -> bool {
    text.split(['/', '\\']).any(is_codex)
}

/// The files a Codex `apply_patch` patch adds, updates, deletes or moves to.
fn patch_targets(patch: &str) -> Vec<PathBuf> {
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
mod tests;
