//! Which of Claude Code's own network-address asks the host answers for the lawyer: a Read, Grep
//! or Glob that stays, by its text alone, inside the conversation's own matter folder as the
//! matter was registered (same server, same share, same folders, same case). Never touches the
//! disk. Claude Code's ask guards against a path the model picked making Windows sign the lawyer
//! in (NTLM) to a server an outsider runs; the matter's server and share were picked by the
//! lawyer, and Clauseo already opens them by this very text.
//!
//! Strict on purpose, so any doubt leaves Claude Code's ask as it is:
//! - the tool is exactly `Read`, `Grep` or `Glob`, and every input key is one Claude Code 2.1.293
//!   gives that tool (`sdk-tools.d.ts` 0.3.293: `FileReadInput`, `GrepInput`, `GlobInput`);
//! - the path (Read's `file_path`; Grep's and Glob's `path`, else the agent's working folder) is
//!   the matter folder or under it by [`under_share_matter`] with exact case, so no `.` or `..`,
//!   no doubled separator, no trailing dot or space, no `\\?\UNC\` respelling, no other server
//!   spelling (IP, FQDN, `@SSL`, `@port`) and no short name of the matter's own folders;
//! - no name past the matter's holds `:` (a stream), a wildcard or a control character, or is
//!   a device name ([`refused_windows_name`]);
//! - Glob's `pattern` and Grep's `glob` are relative and hold no `..`: Claude Code takes an
//!   absolute Glob pattern's base as the folder it searches.
//!
//! What text cannot show is a link inside the matter. A junction is followed by the server,
//! inside its own disks. A symbolic link on a share is followed by this computer only when
//! Windows' link policy allows remote links, which it does not by default; [`links_followed`]
//! reads that policy, and the host answers nothing while it allows them.

use std::path::Path;

use serde_json::{Map, Value};

use crate::network_paths::{NetEnv, refused_windows_name, under_share_matter};

/// Claude Code's Read tool's input keys.
const READ_KEYS: [&str; 4] = ["file_path", "offset", "limit", "pages"];

/// Claude Code's Grep tool's input keys.
const GREP_KEYS: [&str; 15] = [
    "pattern",
    "path",
    "glob",
    "output_mode",
    "-B",
    "-A",
    "-C",
    "context",
    "-n",
    "-i",
    "-o",
    "type",
    "head_limit",
    "offset",
    "multiline",
];

/// Claude Code's Glob tool's input keys.
const GLOB_KEYS: [&str; 2] = ["pattern", "path"];

/// The characters no name past the matter's may hold: a stream's `:`, the wildcards and `|`.
const ODD: [char; 7] = [':', '*', '?', '"', '<', '>', '|'];

/// Whether Claude Code's ask for tool `tool` with input `input` is one the host answers once,
/// unseen: it reads only inside the matter folder `matter` (as stored, by its network address),
/// `cwd` being the agent's working folder (what Grep and Glob search with no `path`).
pub(crate) fn own_share_read(
    tool: Option<&str>,
    input: Option<&Value>,
    matter: &Path,
    cwd: &Path,
) -> bool {
    let Some(Value::Object(input)) = input else {
        return false;
    };
    match tool {
        Some("Read") => {
            known_keys(input, &READ_KEYS) && string(input, "file_path").is_some_and(inside(matter))
        }
        Some("Grep") => {
            known_keys(input, &GREP_KEYS)
                && string(input, "pattern").is_some()
                && optional(input, "glob", relative_pattern)
                && searched_inside(input, matter, cwd)
        }
        Some("Glob") => {
            known_keys(input, &GLOB_KEYS)
                && string(input, "pattern").is_some_and(relative_pattern)
                && searched_inside(input, matter, cwd)
        }
        _ => false,
    }
}

/// Whether every key of `input` is one of `keys`.
fn known_keys(input: &Map<String, Value>, keys: &[&str]) -> bool {
    input.keys().all(|key| keys.contains(&key.as_str()))
}

/// `input`'s string `key`, if it is one.
fn string<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input.get(key)?.as_str()
}

/// Whether `input`'s `key` is absent, or a string that passes `test`.
fn optional(input: &Map<String, Value>, key: &str, test: fn(&str) -> bool) -> bool {
    match input.get(key) {
        None => true,
        Some(value) => value.as_str().is_some_and(test),
    }
}

/// Whether Grep's or Glob's folder is inside `matter`: its `path`, or `cwd` with none.
fn searched_inside(input: &Map<String, Value>, matter: &Path, cwd: &Path) -> bool {
    match input.get("path") {
        None => cwd.to_str().is_some_and(inside(matter)),
        Some(path) => path.as_str().is_some_and(inside(matter)),
    }
}

/// Whether a path's text is `matter` or under it, strictly (above).
fn inside(matter: &Path) -> impl Fn(&str) -> bool + '_ {
    move |path| {
        let net = strict_net();
        let Some(stored) = matter.to_str() else {
            return false;
        };
        if !under_share_matter(Path::new(path), matter, &net)
            || refused_windows_name(Path::new(path), &net)
        {
            return false;
        }
        names(path)
            .get(names(stored).len()..)
            .unwrap_or_default()
            .iter()
            .all(|name| !name.chars().any(|c| ODD.contains(&c) || c.is_control()))
    }
}

/// Whether a Glob pattern (or Grep's `glob`) stays below the folder searched: not empty, not
/// absolute (`\`, `/`, a drive, `~`), no `:` and no `..` anywhere (brace forms included).
fn relative_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && !pattern.starts_with(['\\', '/', '~'])
        && !pattern.contains(':')
        && !pattern.contains("..")
        && !pattern.chars().any(char::is_control)
}

/// Windows' rules with exact case: a per-folder case-sensitive server folder could hold
/// `matters` beside `Matters`. Its drive answer is never asked (the paths are UNC).
fn strict_net() -> NetEnv {
    NetEnv {
        windows: true,
        fold_case: false,
        remote_mounts: Vec::new(),
        remote_drive: Box::new(|_| true),
        macos: false,
        firmlinks: Some(Vec::new()),
    }
}

/// `text`'s names split at `\` and `/`, one trailing separator dropped, as
/// [`under_share_matter`] splits them.
fn names(text: &str) -> Vec<&str> {
    let separator = |c: char| c == '/' || c == '\\';
    text.strip_suffix(separator)
        .unwrap_or(text)
        .split(separator)
        .collect()
}

/// Whether Windows' link policy, its values as read (`network_paths::link_policy`: `None` when
/// not set, `Err` when unreadable), may follow a link on a share: any value set to other than
/// 0, or one that could not be read. Windows' default (none set) follows none.
pub(crate) fn links_followed(values: &[Result<Option<u32>, ()>]) -> bool {
    values
        .iter()
        .any(|value| !matches!(value, Ok(None | Some(0))))
}

#[cfg(test)]
mod tests;
