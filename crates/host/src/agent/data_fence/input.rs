//! What a permission request names, for the data folder fence: the paths it may touch, the
//! words of its shell command, and the folders it may search the whole of.

use std::path::{Component, PathBuf};

use serde_json::Value;

use super::{ShellKind, command, search};

/// Keys whose text Claude Code's own tools write or run, or treat as a pattern, not as a path:
/// read elsewhere (`command`, a Glob's `pattern`, a Grep's `glob`) or not at all.
const NOT_PATHS: [&str; 10] = [
    "command",
    "content",
    "old_string",
    "new_string",
    "new_source",
    "prompt",
    "description",
    "pattern",
    "glob",
    "plan",
];

/// How deep into `rawInput` and how many strings [`input_paths`] reads.
const MAX_DEPTH: usize = 8;
const MAX_STRINGS: usize = 512;

/// The paths a permission request may touch, as the agent wrote them: `paths` (its locations
/// and diffs), then `rawInput`'s `file_path`, `notebook_path`, `path`, `cwd` and each string
/// of `paths`, then every string anywhere in `rawInput` that looks like a path
/// ([`input_paths`]), then the path tokens of `rawInput.command` ([`command::command_paths`],
/// over [`command_lines`]). Without repeats.
pub(crate) fn candidates(
    paths: &[PathBuf],
    raw_input: Option<&Value>,
    windows: bool,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |path: PathBuf| {
        if !path.as_os_str().is_empty() && !out.contains(&path) {
            out.push(path);
        }
    };
    paths.iter().cloned().for_each(&mut add);
    let Some(input) = raw_input else {
        return out;
    };
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    for key in ["file_path", "notebook_path", "path", "cwd"] {
        text(key).map(PathBuf::from).into_iter().for_each(&mut add);
    }
    let listed = input
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    listed
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .for_each(&mut add);
    input_paths(input, windows).into_iter().for_each(&mut add);
    for line in command_lines(input) {
        command::command_paths(line, windows)
            .into_iter()
            .for_each(&mut add);
    }
    out
}

/// Every word of `rawInput.command` ([`command_lines`]), quotes removed.
pub(crate) fn candidates_words(raw_input: Option<&Value>, windows: bool) -> Vec<String> {
    let lines = raw_input.map(command_lines).unwrap_or_default();
    lines
        .into_iter()
        .flat_map(|line| command::command_words(line, windows))
        .collect()
}

/// How many words of a Codex command [`command_args`] takes, at most.
const MAX_ARGS: usize = 256;

/// The words of a Codex command (`rawInput.command`, [`command_lines`]) that may name a file,
/// taken from `rawInput.cwd` when it is absolute: each word that is no option (`-x`), the
/// value of a `name=value` (`--target=out`, `dd`'s `if=link`), and nothing that a shell would
/// expand or that is a URL (`$`, `%`, a backtick, `*`, `?`, `[`, `://`), which the check could
/// only guess at. A bare name such as
/// `link.txt` counts, unlike [`candidates`]'s path tokens: a link in the matter folder may lead
/// anywhere. Best effort: a script can still build a path the words do not show.
pub(crate) fn command_args(raw_input: Option<&Value>, windows: bool) -> Vec<PathBuf> {
    let Some(input) = raw_input else {
        return Vec::new();
    };
    let cwd = command_cwd(raw_input);
    let mut out: Vec<PathBuf> = Vec::new();
    for word in command_lines(input)
        .into_iter()
        .flat_map(|line| command::command_words(line, windows))
    {
        let word = match word.split_once('=') {
            Some((name, value)) if !name.contains(['/', '\\']) => value.to_owned(),
            _ => word,
        };
        let expands = ['$', '%', '`', '*', '?', '[']
            .iter()
            .any(|c| word.contains(*c));
        if word.is_empty() || word.starts_with('-') || expands || word.contains("://") {
            continue;
        }
        let path = match &cwd {
            Some(cwd) if !word.starts_with('~') => cwd.join(&word),
            _ => PathBuf::from(&word),
        };
        if !out.contains(&path) {
            out.push(path);
        }
        if out.len() >= MAX_ARGS {
            break;
        }
    }
    out
}

/// A Codex request's `rawInput.cwd`, where its command runs: an absolute one as it is, or a
/// relative one (`sub`, with no drive or root) left relative, so that a path joined to it is
/// still taken from the matter folder, under `sub`.
pub(crate) fn command_cwd(raw_input: Option<&Value>) -> Option<PathBuf> {
    raw_input?
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| {
            cwd.is_absolute()
                || (cwd.components().next().is_some()
                    && cwd.components().all(|part| {
                        matches!(
                            part,
                            Component::Normal(_) | Component::CurDir | Component::ParentDir
                        )
                    }))
        })
}

/// `paths` (a request's paths or search folders) with each relative one taken from Codex's
/// `rawInput.cwd` ([`command_cwd`]), where its command runs, instead of the matter folder; as
/// they are when there is none. `~` forms stay as written, and so does a path starting with a
/// variable (`$HOME/…`, PowerShell's `$env:USERPROFILE\…`, cmd's `%USERPROFILE%\…`): the
/// variable names its own folder, not one under the command's. The data-folder check judges a
/// PowerShell variable where it leads (`variables::judged`); cmd's `%USERPROFILE%` stays
/// unhandled until #1085's `variables::judged` (rebase).
pub(crate) fn from_command_cwd(raw_input: Option<&Value>, paths: &mut [PathBuf]) {
    let Some(cwd) = command_cwd(raw_input) else {
        return;
    };
    for path in paths {
        let variable = path
            .to_str()
            .is_some_and(|text| text.starts_with(['$', '%']));
        if path.is_relative() && !path.starts_with("~") && !variable {
            *path = cwd.join(&*path);
        }
    }
}

/// The folders a request may search the whole of ([`search::search_roots`]), its command read
/// as `shell`, the shell of the tool that sent it. Without repeats.
pub(crate) fn candidates_roots(
    raw_input: Option<&Value>,
    windows: bool,
    shell: ShellKind,
) -> Vec<PathBuf> {
    let Some(input) = raw_input else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for root in search::search_roots(input, &command_lines(input), windows, shell) {
        if !out.contains(&root) {
            out.push(root);
        }
    }
    out
}

/// The command names of `rawInput.command` ([`command::command_names`]): a string as one
/// line; an array (a program and its arguments) quoted back into one, so an argument such as
/// `echo`'s `curl` is never taken for a command, while `bash -lc '<script>'` still is read.
pub(crate) fn command_names(raw_input: Option<&Value>, windows: bool) -> Vec<String> {
    let line = match raw_input.and_then(|input| input.get("command")) {
        Some(Value::String(line)) => line.clone(),
        Some(Value::Array(words)) => {
            let quoted = words.iter().filter_map(Value::as_str).map(|word| {
                match (word.contains('\''), word.contains('"')) {
                    (false, _) => format!("'{word}'"),
                    (true, false) => format!("\"{word}\""),
                    (true, true) => word.to_owned(),
                }
            });
            quoted.collect::<Vec<_>>().join(" ")
        }
        _ => return Vec::new(),
    };
    command::command_names(&line, windows)
}

/// `rawInput.command`'s command lines: the string, or each string of an array, as Codex sends
/// `["bash", "-lc", "…"]`.
fn command_lines(input: &Value) -> Vec<&str> {
    match input.get("command") {
        Some(Value::String(line)) => vec![line.as_str()],
        Some(Value::Array(parts)) => parts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Every string in `input`, at any depth (to [`MAX_DEPTH`], the first [`MAX_STRINGS`]), under
/// any key but [`NOT_PATHS`], that looks like a path ([`as_path`]): so a third-party tool's
/// `filename`, `uri` or nested `target` is read, and free text is not.
pub(crate) fn input_paths(input: &Value, windows: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = 0;
    let mut stack = vec![(input, 0usize)];
    while let Some((value, depth)) = stack.pop() {
        match value {
            Value::String(text) => {
                seen += 1;
                out.extend(as_path(text, windows));
            }
            Value::Array(items) if depth < MAX_DEPTH => {
                stack.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Value::Object(map) if depth < MAX_DEPTH => {
                let kept = map
                    .iter()
                    .filter(|(key, _)| !NOT_PATHS.contains(&key.as_str()));
                stack.extend(kept.map(|(_, item)| (item, depth + 1)));
            }
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::Array(_)
            | Value::Object(_) => {}
        }
        if seen >= MAX_STRINGS {
            break;
        }
    }
    out
}

/// `text` as a path when it looks like one: absolute (`/…`, and on Windows `\\…` or a
/// drive), from a home (`~`, `$HOME`), relative from `./` or `../`, or a `file://` URL (its
/// path, `localhost` dropped). Not a word, a sentence or a web URL.
fn as_path(text: &str, windows: bool) -> Option<PathBuf> {
    let text = text.trim();
    let text = match text.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file://") => {
            let rest = text.get(7..).unwrap_or_default();
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            let mut chars = rest.chars();
            let drive = matches!(
                (chars.next(), chars.next(), chars.next()),
                (Some('/'), Some(letter), Some(':')) if letter.is_ascii_alphabetic()
            );
            if windows && drive {
                rest.get(1..).unwrap_or(rest)
            } else {
                rest
            }
        }
        _ => text,
    };
    let mut chars = text.chars();
    let drive = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('\\' | '/')) if letter.is_ascii_alphabetic()
    );
    let shaped = text.starts_with('/')
        || text.starts_with('~')
        || text.starts_with("$HOME")
        || text.starts_with("${HOME}")
        || text.starts_with("./")
        || text.starts_with("../")
        || (windows && (drive || text.starts_with('\\')));
    shaped.then(|| PathBuf::from(command::home_as_tilde(text)))
}
