//! A shell command's targets: its words, segment by segment, each candidate name taken from the
//! hook's `cwd` and from every folder the command's own folder changes may have reached by then
//! (by the folder's text, as the shell's default `cd` does, and through symlinks, as `cd -P`
//! does).

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::words::{Escape, Segments, candidates, shell_segments};
use super::{
    Ignored, MAX_WORDS, Targets, existing_resolved, lexical, patch_targets, text_has_codex_part,
};

/// The most folders [`Folders`] keeps; past it, the folder is unknown.
const MAX_FOLDERS: usize = 16;

/// The folders a command may be in after its segments so far.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Folders {
    /// The hook's `cwd`, then every folder a folder change may have reached. A change that did not
    /// run (`false && cd x`, a subshell's `cd`) only adds a folder, so the real one is always here
    /// unless `unknown`.
    reached: Vec<PathBuf>,
    /// Whether a folder change went somewhere the guard cannot follow (`cd "$(…)"`, `cd ~`, `cd -`,
    /// `popd` with nothing pushed), or more than [`MAX_FOLDERS`] were reached. It stays set.
    unknown: bool,
    /// How many `pushd`s the command ran, for `popd`.
    pushed: usize,
    /// The folder if every folder change ran, in order, by the folder's text, whatever
    /// [`MAX_FOLDERS`]; `None` after a change it cannot follow, until an absolute one.
    linear: Option<PathBuf>,
    /// `linear` before each `pushd`, for `popd`.
    linear_pushed: Vec<Option<PathBuf>>,
}

/// One segment to read: its words, whether it is a command (not a heredoc body), and its shell's
/// escape character.
struct Part<'a> {
    words: &'a [String],
    command: bool,
    escape: Escape,
}

/// A shell command's targets: the existing files among its first [`MAX_WORDS`] words (and the
/// first [`MAX_WORDS`] of each script it hands to `sh -c`, `bash -c`, `eval`, `pwsh -Command` and
/// the like), the files of a patch in its text, and whether one of those words has a `.codex`
/// part. Claude Code sends the command as a string; an argument list (one shape Codex has used) is
/// taken word by word, each split again so `["bash", "-lc", "…"]` reaches the script's words.
///
/// Each word's candidates ([`candidates`]) are taken from `cwd` and from every folder reached so
/// far ([`Folders`], [`moved`]). When the command names a folder change anywhere, followed or
/// not, a relative candidate is also taken from `cwd` with its leading `..` and `.` parts off: a
/// backstop for changes the guard misreads, at the cost of a rare refusal of a write that lands
/// elsewhere. Each path counts as spelled and resolved ([`resolutions`]). A command of another
/// JSON type is ignored.
pub(super) fn shell_targets(
    command: Option<&Value>,
    cwd: &Path,
    escape: Escape,
) -> Result<Targets, Ignored> {
    let mut segments = Segments::new(MAX_WORDS);
    let mut text = String::new();
    match command {
        Some(Value::String(command)) => {
            shell_segments(command, escape, &mut segments);
            text.push_str(command);
        }
        Some(Value::Array(parts)) => {
            for part in parts.iter().filter_map(Value::as_str) {
                text.push_str(part);
                text.push('\n');
                segments.split();
                segments.push(part.to_owned());
                shell_segments(part, escape, &mut segments);
            }
        }
        _ => return Err(Ignored::Shape("command string or list")),
    }
    let scripts: Vec<(Segments, Escape)> = segments
        .iter()
        .filter(|segment| segment.command)
        .filter_map(|segment| script(&segment.words, escape))
        .map(|(script, escape)| {
            let mut inner = Segments::new(MAX_WORDS);
            shell_segments(&script, escape, &mut inner);
            (inner, escape)
        })
        .collect();
    let parts: Vec<Part<'_>> = segments
        .iter()
        .map(|segment| (segment, escape))
        .chain(
            scripts
                .iter()
                .flat_map(|(inner, escape)| inner.iter().map(move |segment| (segment, *escape))),
        )
        .map(|(segment, escape)| Part {
            words: &segment.words,
            command: segment.command,
            escape,
        })
        .collect();
    let changes = parts
        .iter()
        .any(|part| part.words.iter().any(|word| change(word).is_some()));
    let mut files: Vec<PathBuf> = Vec::new();
    let mut names_codex = false;
    let mut folders = Folders {
        reached: vec![cwd.to_path_buf()],
        unknown: false,
        pushed: 0,
        linear: Some(cwd.to_path_buf()),
        linear_pushed: Vec::new(),
    };
    for part in &parts {
        for word in part.words {
            for candidate in candidates(word) {
                names_codex |= text_has_codex_part(candidate);
                let candidate = native(candidate, part.escape);
                for path in starts(&folders, changes, Path::new(candidate.as_ref())) {
                    for path in resolutions(&path) {
                        if !files.contains(&path) && path.metadata().is_ok_and(|m| m.is_file()) {
                            files.push(path);
                        }
                    }
                }
            }
        }
        if part.command {
            moved(&mut folders, part.words, part.escape);
        }
    }
    Ok(Targets::Shell {
        files,
        patch: patch_targets(&text),
        names_codex,
        text,
        escape,
    })
}

/// `path` as spelled when it has no `.` or `..` part; else `path` resolved two ways through its
/// deepest existing folder: as spelled (a symlinked folder's `..` is its target's parent, as the
/// OS reads it; when the path exists this is the file the OS opens, so the spelling itself is not
/// checked again), and with `..` and `.` resolved by their text first (`mkdir -p D && echo x >
/// D/../Contract.docx`, or a `D` that never exists).
pub(super) fn resolutions(path: &Path) -> Vec<PathBuf> {
    let normal = lexical(path);
    if normal == path {
        return vec![normal];
    }
    let spelled = existing_resolved(path);
    let normal = existing_resolved(&normal);
    if spelled == normal {
        vec![spelled]
    } else {
        vec![spelled, normal]
    }
}

/// `candidate` with its separators as the OS reads them: PowerShell takes `\` as a separator on
/// every OS, so off Windows it becomes `/`.
fn native(candidate: &str, escape: Escape) -> Cow<'_, str> {
    if escape == Escape::Backtick && !cfg!(windows) && candidate.contains('\\') {
        Cow::Owned(candidate.replace('\\', "/"))
    } else {
        Cow::Borrowed(candidate)
    }
}

/// The paths `candidate` may name: from each folder reached, and when the command changes folder
/// (`changes`) or went somewhere unknown, from the first (`cwd`) with its leading `..` and `.`
/// parts off (`cd "$(…)" && echo x > ../Contract.docx` most likely went one folder down first).
fn starts(folders: &Folders, changes: bool, candidate: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = folders
        .reached
        .iter()
        .chain(
            folders
                .linear
                .iter()
                .filter(|linear| !folders.reached.contains(linear)),
        )
        .map(|folder| folder.join(candidate))
        .collect();
    if (changes || folders.unknown)
        && let Some(cwd) = folders.reached.first()
    {
        let rest: PathBuf = candidate
            .components()
            .skip_while(|part| matches!(part, Component::ParentDir | Component::CurDir))
            .collect();
        if !rest.as_os_str().is_empty() && rest != candidate {
            paths.push(cwd.join(rest));
        }
    }
    paths
}

/// What a folder-changing command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// `cd`, `chdir`, `Set-Location`, `sl`; with `push`, `pushd` and `Push-Location`.
    Move { push: bool },
    /// `popd`, `Pop-Location`.
    Pop,
}

/// The folder change `word` names, ignoring case and shell punctuation around it.
fn change(word: &str) -> Option<Change> {
    let name = word
        .trim_start_matches(['{', '(', '!', '$', '`', '@', '&'])
        .trim_end_matches([';', ')', '}'])
        .to_ascii_lowercase();
    match name.as_str() {
        "cd" | "chdir" | "set-location" | "sl" | "cd.." | "cd\\" => {
            Some(Change::Move { push: false })
        }
        "pushd" | "push-location" => Some(Change::Move { push: true }),
        "popd" | "pop-location" => Some(Change::Pop),
        _ => None,
    }
}

/// `words` from the command name on: leading reserved words, braces, `!`, the `builtin`,
/// `command` and `time` prefixes and `NAME=value` assignments are skipped.
pub(super) fn command_words(words: &[String]) -> &[String] {
    let skip = words
        .iter()
        .take_while(|word| {
            matches!(
                word.as_str(),
                "{" | "}"
                    | "("
                    | "!"
                    | "if"
                    | "then"
                    | "do"
                    | "else"
                    | "elif"
                    | "while"
                    | "until"
                    | "time"
                    | "builtin"
                    | "command"
            ) || is_assignment(word)
        })
        .count();
    words.get(skip..).unwrap_or_default()
}

/// Whether `word` is a shell variable assignment, `NAME=value`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The script a segment hands to another shell, and that shell's escape: the argument after a
/// `-c`-style option of `sh`, `bash`, `zsh`, `dash` or `ksh` (`-c`, `-lc`, `-ec`), the arguments
/// after `-c` or `-Command` of `pwsh` or `powershell`, or the arguments of `eval` (in this
/// shell). One level only: a script's own scripts are not read.
pub(super) fn script(words: &[String], escape: Escape) -> Option<(String, Escape)> {
    let (program, args) = command_words(words).split_first()?;
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => {
            let at = args.iter().position(|arg| {
                arg.strip_prefix('-')
                    .is_some_and(|flags| !flags.starts_with('-') && flags.contains('c'))
            })?;
            Some((args.get(at + 1)?.clone(), Escape::Backslash))
        }
        "pwsh" | "powershell" => {
            let at = args.iter().position(|arg| {
                matches!(
                    arg.to_ascii_lowercase().as_str(),
                    "-c" | "-command" | "/c" | "/command"
                )
            })?;
            Some((args.get(at + 1..)?.join(" "), Escape::Backtick))
        }
        "eval" | "invoke-expression" | "iex" => Some((args.join(" "), escape)),
        _ => None,
    }
}

/// `folders` after the segment `words` runs. A segment that changes folder ([`change`], after
/// [`command_words`]) names its folder by its first word not starting with `-`, or the part after
/// `:` of a `-Path:x` word. Its folder joined to each folder reached, by its text and through
/// symlinks, is added; an absolute folder is added alone. No folder, `-`, `+N`, or one starting
/// with `~` (home is usually above the matter) or holding `$`, `` ` ``, a glob or a brace is
/// unknown. `popd` and `Pop-Location` go back to a folder already reached, or to an unknown one
/// when the command pushed none.
fn moved(folders: &mut Folders, words: &[String], escape: Escape) {
    let Some((command, args)) = command_words(words).split_first() else {
        return;
    };
    let push = match change(command) {
        Some(Change::Move { push }) => push,
        Some(Change::Pop) => {
            if folders.pushed == 0 {
                folders.unknown = true;
            } else {
                folders.pushed -= 1;
            }
            folders.linear = folders.linear_pushed.pop().flatten();
            return;
        }
        None => return,
    };
    if push {
        folders.pushed += 1;
        folders.linear_pushed.push(folders.linear.clone());
    }
    // PowerShell's built-in `cd..` and `cd\`.
    let name = command.to_ascii_lowercase();
    let target = match name.trim_start_matches(['{', '(', '!', '$', '`', '@', '&']) {
        "cd.." => Some(".."),
        "cd\\" => Some("\\"),
        _ => args.iter().find_map(|arg| match arg.strip_prefix('-') {
            Some(option) if !option.is_empty() => option
                .split_once(':')
                .map(|(_, value)| value.trim_matches(['\'', '"'])),
            Some(_) | None => Some(arg.as_str()),
        }),
    };
    let target = target.map(|target| target.trim_end_matches([')', '}', ';']));
    let Some(target) = target.filter(|target| {
        !(target.is_empty()
            || *target == "-"
            || target.starts_with(['~', '+'])
            || target.contains(['$', '`', '*', '?', '[', '{']))
    }) else {
        folders.unknown = true;
        folders.linear = None;
        return;
    };
    let target = native(target, escape);
    let target = Path::new(target.as_ref());
    folders.linear = if target.is_absolute() {
        Some(lexical(target))
    } else {
        folders
            .linear
            .as_ref()
            .map(|linear| lexical(&linear.join(target)))
    };
    let joined: Vec<PathBuf> = if target.is_absolute() {
        vec![target.to_path_buf()]
    } else {
        folders
            .reached
            .iter()
            .map(|folder| folder.join(target))
            .collect()
    };
    for path in joined {
        for folder in [lexical(&path), existing_resolved(&path)] {
            if folders.reached.contains(&folder) {
                continue;
            }
            if folders.reached.len() >= MAX_FOLDERS {
                folders.unknown = true;
                return;
            }
            folders.reached.push(folder);
        }
    }
}

#[cfg(test)]
mod tests;
