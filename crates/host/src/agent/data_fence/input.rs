//! What a permission request names, for the data folder fence: the paths it may touch, the
//! words of its shell command, and the folders it may search the whole of.

use std::path::PathBuf;

use serde_json::Value;

use super::{command, search};

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

/// The folders a request may search the whole of ([`search::search_roots`]). Without repeats.
pub(crate) fn candidates_roots(raw_input: Option<&Value>, windows: bool) -> Vec<PathBuf> {
    let Some(input) = raw_input else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for root in search::search_roots(input, &command_lines(input), windows) {
        if !out.contains(&root) {
            out.push(root);
        }
    }
    out
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
