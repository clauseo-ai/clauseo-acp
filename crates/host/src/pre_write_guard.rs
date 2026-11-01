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
//! - `Bash` (both agents): a heuristic. `tool_input.command` is split into words the simple shell
//!   way (whitespace, quotes and backslashes; no expansion), the first [`MAX_WORDS`] are kept,
//!   and a word names a target when it is an existing file. A word's part after its last `>` or
//!   `<` (a redirection such as `>Contract.docx`) and after its first `=` (`--out=Contract.docx`)
//!   count too. A command that reaches a file another way (a glob, a variable, a script) is not
//!   caught; the permission request remains.
//!
//! Each target is taken from the hook's `cwd`, the session folder, which is the matter folder,
//! and checked with [`clauseo_matters::held_open_at`], the permission check's own rule: both
//! sides resolved canonically (symlinks, `..`, case on macOS and Windows), a target outside the
//! folder not checked. The first one held open gets the deny object on `output`.
//!
//! One more rule for Codex's patches alone: a target in the matter's `.codex` folder (the folder
//! or anything under it; every path the patch names, a move's source and destination both) is
//! refused, held open or not. codex-acp trusts the session folder, so a
//! `.codex/config.toml` there loads, and in workspace-write Codex could write one without asking
//! (clauseo-acp-private#184). A target counts when either resolution puts it there:
//! [`clauseo_matters::matter_path_in`] (canonical, so a symlink to the folder is caught) or a
//! lexical one (`..` takes a part off, so `sub/../.codex/x` is caught even with no `sub` yet,
//! which Codex would create). The name compares ignoring ASCII case and trailing dots and
//! spaces, as macOS and Windows file systems do. Claude Code needs no such rule: its
//! `settingSources` leave out project settings.
//!
//! It fails open: input it cannot read or parse, an unknown tool, a missing `cwd`, gets nothing
//! on `output`, and the tool goes on. Refusing every tool when an agent changes its input shape
//! would stop the agent outright, and the refusal at the permission request remains. It always
//! returns 0 (an exit code), never panics, and writes nothing but the deny object.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterPath;
use serde_json::Value;

/// The first argument that makes the desktop binary run [`run`] on its stdin and stdout and exit,
/// before anything else starts. The hook entry passes it (`agent/loads.rs`).
pub const FLAG: &str = "--pre-write-guard";

/// How many of a `Bash` command's words are looked at.
pub const MAX_WORDS: usize = 64;

/// The most input read: a hook object is a few kilobytes, a patch rarely more than a megabyte.
/// Anything longer does not parse and fails open.
const MAX_INPUT: u64 = 16 * 1024 * 1024;

/// The guard: one hook input object from `input`; the deny object on `output` when a target is
/// held open, else nothing. Returns the exit code, always 0.
pub fn run(input: impl Read, mut output: impl Write) -> i32 {
    let mut bytes = Vec::new();
    if input.take(MAX_INPUT).read_to_end(&mut bytes).is_err() {
        return 0;
    }
    let Some(refusal) = refusal(&bytes) else {
        return 0;
    };
    let deny = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": refusal.reason(),
        }
    });
    if let Ok(mut line) = serde_json::to_vec(&deny) {
        line.push(b'\n');
        // Nothing more can be done when the agent stopped reading; the tool then goes on.
        drop(output.write_all(&line).and_then(|()| output.flush()));
    }
    0
}

/// Why the guard refuses a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    /// A target is held open by Word or another program.
    HeldOpen(MatterPath),
    /// A Codex patch writes in the matter's `.codex` folder.
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

/// Why the hook input `bytes` is refused, if it is: a Codex patch target in `<cwd>/.codex`,
/// else the first target that lies in its `cwd` and is held open.
fn refusal(bytes: &[u8]) -> Option<Refusal> {
    let hook: Value = serde_json::from_slice(bytes).ok()?;
    let cwd = PathBuf::from(hook.get("cwd")?.as_str()?);
    if !cwd.is_absolute() {
        return None;
    }
    let tool = hook.get("tool_name")?.as_str()?;
    let input = hook.get("tool_input")?;
    let targets = targets(tool, input, &cwd);
    if let Targets::Patch(paths) = &targets
        && paths.iter().any(|path| in_codex_folder(&cwd, path))
    {
        return Some(Refusal::CodexFolder);
    }
    targets
        .paths()
        .iter()
        .find_map(|path| clauseo_matters::held_open_at(&cwd, path))
        .map(|(path, _)| Refusal::HeldOpen(path))
}

/// The paths a tool would write, as the agent spelled them, and where they came from.
enum Targets {
    /// A file tool's path, or the files a shell command names.
    Named(Vec<PathBuf>),
    /// The files of a Codex `apply_patch` patch.
    Patch(Vec<PathBuf>),
}

impl Targets {
    fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Named(paths) | Self::Patch(paths) => paths,
        }
    }
}

/// The paths the tool `tool` with `input` would write.
fn targets(tool: &str, input: &Value, cwd: &Path) -> Targets {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
            if let Some(path) = text("file_path").or_else(|| text("notebook_path")) {
                Targets::Named(vec![PathBuf::from(path)])
            } else if let Some(patch) = text("command") {
                Targets::Patch(patch_targets(patch))
            } else {
                Targets::Named(Vec::new())
            }
        }
        "Bash" => Targets::Named(bash_targets(input.get("command"), cwd)),
        _ => Targets::Named(Vec::new()),
    }
}

/// Whether `path`, taken from `cwd`, is `<cwd>/.codex` or lies under it, resolved canonically
/// or lexically. The folder's name compares ignoring ASCII case and trailing dots and spaces.
fn in_codex_folder(cwd: &Path, path: &Path) -> bool {
    let is_codex = |name: &str| {
        name.trim_end_matches(['.', ' '])
            .eq_ignore_ascii_case(".codex")
    };
    if let Some(inside) = clauseo_matters::matter_path_in(cwd, path)
        && inside.as_str().split('/').next().is_some_and(is_codex)
    {
        return true;
    }
    let mut resolved = PathBuf::new();
    for part in cwd.join(path).components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => resolved.push(part),
        }
    }
    let Ok(relative) = resolved.strip_prefix(cwd) else {
        return false;
    };
    relative
        .components()
        .next()
        .is_some_and(|first| first.as_os_str().to_str().is_some_and(is_codex))
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

/// The existing files a shell command names among its first [`MAX_WORDS`] words. Claude Code
/// sends the command as a string; an argument list (one shape Codex has used) is taken word by
/// word, each split again so `["bash", "-lc", "…"]` reaches the script's words.
fn bash_targets(command: Option<&Value>, cwd: &Path) -> Vec<PathBuf> {
    let mut words: Vec<String> = Vec::new();
    match command {
        Some(Value::String(command)) => shell_words(command, MAX_WORDS, &mut words),
        Some(Value::Array(parts)) => {
            for part in parts.iter().filter_map(Value::as_str) {
                if words.len() >= MAX_WORDS {
                    break;
                }
                words.push(part.to_owned());
                shell_words(part, MAX_WORDS, &mut words);
            }
            words.truncate(MAX_WORDS);
        }
        _ => {}
    }
    let mut targets: Vec<PathBuf> = Vec::new();
    for word in &words {
        for candidate in candidates(word) {
            let path = cwd.join(candidate);
            if !targets.contains(&path) && path.metadata().is_ok_and(|meta| meta.is_file()) {
                targets.push(path);
            }
        }
    }
    targets
}

/// `word`, its part after the last `>` or `<`, and its part after the first `=`, each when not
/// empty.
fn candidates(word: &str) -> impl Iterator<Item = &str> {
    let redirected = word
        .rfind(['>', '<'])
        .and_then(|at| word.get(at + 1..))
        .filter(|rest| rest.len() < word.len());
    let assigned = word.split_once('=').map(|(_, value)| value);
    [Some(word), redirected, assigned]
        .into_iter()
        .flatten()
        .filter(|candidate| !candidate.is_empty())
}

/// Appends `command`'s words to `words` until it holds `limit`: split at unquoted whitespace;
/// single quotes keep everything; double quotes keep everything but a backslash before `"`,
/// `\`, `$` or `` ` ``; a backslash outside quotes keeps the next character. No expansion, no
/// operators: `a;b` is one word. An unclosed quote ends with the command.
pub(crate) fn shell_words(command: &str, limit: usize, words: &mut Vec<String>) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut quote = Quote::None;
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if words.len() >= limit {
            return;
        }
        match (quote, c) {
            (Quote::None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (Quote::None, '\'') => {
                quote = Quote::Single;
                in_word = true;
            }
            (Quote::None, '"') => {
                quote = Quote::Double;
                in_word = true;
            }
            (Quote::None, '\\') => {
                in_word = true;
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::None,
            (Quote::Double, '\\') => match chars.peek() {
                Some(&next @ ('"' | '\\' | '$' | '`')) => {
                    word.push(next);
                    chars.next();
                }
                _ => word.push('\\'),
            },
            (Quote::None | Quote::Single | Quote::Double, c) => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word && words.len() < limit {
        words.push(word);
    }
}

#[cfg(test)]
mod tests;
