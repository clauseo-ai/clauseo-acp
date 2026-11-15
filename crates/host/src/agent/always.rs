//! "Always for this matter" on a Claude permission request, route A: the host keeps the rule
//! itself instead of letting claude-agent-acp write it into the matter folder.
//!
//! Why: claude-agent-acp 0.89.1 answers its `allow-with-updates` option (kind `allow_always`)
//! by handing Claude Code the CLI's own suggestions (`permissions/effects.js`
//! `applyCommonSelection`), which name `destination: "localSettings"`, so Claude Code writes the
//! rule into `<matter>/.claude/settings.local.json`, the lawyer's own folder. The client picks
//! only an option id, never the destination, and the suggestions never reach it: the request
//! carries the tool call (with `rawInput`) and the options, and the rule shows only in the
//! option's label (`permissions/options/shell.js`). So the turn path answers the agent with the
//! request's `allow_once` option instead (`actor/turn/always.rs`) and keeps the rule here:
//!
//! - [`rule`]: the narrowest rule for the request, in Claude Code's permission rule syntax, from
//!   the tool's name and `rawInput` alone: `Bash(<exact command>)`, `WebFetch(domain:<host>)`
//!   (any page on that site), an MCP tool's exact name, `Edit(//<path>)` for `Edit`, `Write`
//!   and `MultiEdit`, `Read(//<path>)`. Anything else, and any text that would not stay one
//!   exact rule (a `)`, a glob character, a relative path), gives none, with why ([`NotKept`]):
//!   the request is still answered once, and nothing is kept.
//! - [`within`] and [`real_within`]: a path rule is kept and applied only for a file inside the
//!   matter folder, by its text and by its real path (links resolved), never in `.clauseo` or a
//!   `.claude` folder.
//! - The file, `<data>/agent-state/<matter>/claude-allow.json` ([`file_beside`], [`store`]),
//!   beside the matter's notes and not in them: Claude may write the notes folder
//!   (`additionalDirectories`), while this file is in the data fence's fixed deny rules
//!   (`data_fence::DENIED`) and outside every folder the auto-deny counts as the session's own.
//!   Nothing is ever written in the matter folder, and a `.claude` folder the lawyer made there
//!   is left as it is.
//!
//! The kept rules never reach Claude Code's own settings: Claude Code would then allow a matching
//! call itself, past the host's data fence, its held-open check and its log. They answer only
//! the requests Claude Code sends the host, after those checks, by exact match
//! (`actor/turn/always.rs`).

use std::path::{Path, PathBuf};

use serde_json::Value;

mod store;

pub(crate) use store::{list, next_seq, read_beside, remove_now, store};
#[cfg(test)]
pub(crate) use store::{read, store_now};

/// The file's name, in `<data>/agent-state/<matter>`.
pub(crate) const FILE: &str = "claude-allow.json";

/// The most rules a matter keeps; an answer past it keeps nothing more.
pub(crate) const MAX_RULES: usize = 500;

/// The longest rule kept, in bytes.
const MAX_RULE_LEN: usize = 2_000;

/// Why "Always for this matter" kept nothing: the request was allowed this once only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotKept {
    /// The tool has no rule form Clauseo keeps (a shell other than Bash, a tool it does not know).
    NoRule,
    /// Its command, address or path would not stay one exact rule (a wildcard, a bracket, a line
    /// break, a relative path).
    Refused,
    /// A file outside the matter folder, or in its `.clauseo` or a `.claude` folder.
    OutsideMatter,
    /// The matter keeps the most rules it may (`MAX_RULES`).
    Full,
}

/// The rule a request keeps, from its tool's `name` (the tool call's, as the agent sent it) and
/// its `rawInput`, or why none (see the module doc). `windows`: a file path is a Windows one.
pub(crate) fn rule(
    tool: &str,
    raw_input: Option<&Value>,
    windows: bool,
) -> Result<String, NotKept> {
    let text = |key: &str| {
        raw_input
            .and_then(|input| input.get(key)?.as_str())
            .ok_or(NotKept::Refused)
    };
    let exact = |made: Option<String>| made.ok_or(NotKept::Refused);
    let rule = match tool {
        "Bash" => format!(
            "Bash({})",
            exact(command(text("command")?).map(str::to_owned))?
        ),
        "WebFetch" => format!("WebFetch(domain:{})", exact(domain(text("url")?))?),
        "Edit" | "Write" | "MultiEdit" => {
            format!("Edit({})", exact(path(text("file_path")?, windows))?)
        }
        "Read" => format!("Read({})", exact(path(text("file_path")?, windows))?),
        name if is_mcp_tool(name) => name.to_owned(),
        _ => return Err(NotKept::NoRule),
    };
    if rule.len() <= MAX_RULE_LEN {
        Ok(rule)
    } else {
        Err(NotKept::Refused)
    }
}

/// The file a path rule names, as an absolute path of this system (`//c/a` is `c:/a` on
/// Windows); none for any other rule.
pub(crate) fn rule_path(rule: &str, windows: bool) -> Option<PathBuf> {
    let content = ["Edit(", "Read("]
        .iter()
        .find_map(|tool| rule.strip_prefix(tool))
        .and_then(|rest| rest.strip_suffix(')'))?;
    let rest = content.strip_prefix("//")?;
    if windows {
        let (drive, rest) = rest.split_once('/')?;
        Some(PathBuf::from(format!("{drive}:/{rest}")))
    } else {
        Some(PathBuf::from(format!("/{rest}")))
    }
}

/// Whether `rule` is one [`rule`] could have made: a kept rule read back is checked again.
pub(crate) fn well_formed(rule: &str) -> bool {
    if rule.len() > MAX_RULE_LEN {
        return false;
    }
    if is_mcp_tool(rule) {
        return true;
    }
    let Some((tool, content)) = rule.strip_suffix(')').and_then(|rest| rest.split_once('(')) else {
        return false;
    };
    match tool {
        "Bash" => command(content).is_some(),
        "WebFetch" => content
            .strip_prefix("domain:")
            .is_some_and(|host| domain_name(host) && !host.is_empty()),
        "Edit" | "Read" => content
            .strip_prefix('/')
            .is_some_and(|rest| path(rest, false).as_deref() == Some(content)),
        _ => false,
    }
}

/// A shell command as an exact `Bash(…)` rule's content: not empty, no space at either end,
/// and none of `(`, `)` (they end the rule), `*` (a wildcard in Claude Code's Bash rules),
/// `\` (an escape there) or a control character such as a line break.
fn command(command: &str) -> Option<&str> {
    let plain = !command.is_empty()
        && command.trim() == command
        && !command
            .chars()
            .any(|c| matches!(c, '(' | ')' | '*' | '\\') || c.is_control());
    plain.then_some(command)
}

/// The host of an `http` or `https` URL, lower case, for a `WebFetch(domain:…)` rule; none for
/// another scheme, no host, or a host that is not a plain name or IPv4 address.
fn domain(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    domain_name(&host).then_some(host)
}

/// Letters, digits, `.` and `-` only, not starting with `.` or `-`.
fn domain_name(host: &str) -> bool {
    !host.starts_with(['.', '-'])
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-'))
}

/// An MCP tool's exact name, `mcp__<server>__<tool>`: letters, digits, `_` and `-` only.
fn is_mcp_tool(name: &str) -> bool {
    name.strip_prefix("mcp__").is_some_and(|rest| {
        rest.split_once("__")
            .is_some_and(|(server, tool)| !server.is_empty() && !tool.is_empty())
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    })
}

/// An absolute file path as a path rule's content in Claude Code's syntax, `//<path>` (a path
/// from the file system's root; a single `/` would be relative to the settings file). On
/// Windows, `C:\a\b` is `//c/a/b`, as Claude Code turns Windows paths into POSIX ones before it
/// matches. None for a relative path, a UNC path, an empty, `.` or `..` component, or a name
/// with a character that is not literal in Claude Code's gitignore patterns (`*`, `?`, `[`, `]`,
/// `{`, `}`, `!`, `\`), a parenthesis, a control character, or a space at either end; and on
/// Windows a name that ends in `.` (Windows drops trailing dots and spaces, so `.claude.` opens
/// `.claude`).
fn path(path: &str, windows: bool) -> Option<String> {
    let rest = if windows {
        let mut chars = path.chars();
        let (drive, colon) = (chars.next()?, chars.next()?);
        let rest = chars.as_str();
        if !drive.is_ascii_alphabetic() || colon != ':' || !rest.starts_with(['\\', '/']) {
            return None;
        }
        format!("{}{}", drive.to_ascii_lowercase(), rest.replace('\\', "/"))
    } else {
        path.strip_prefix('/')?.to_owned()
    };
    let literal = |name: &str| {
        !matches!(name, "" | "." | "..")
            && name.trim() == name
            && (!windows || !name.ends_with('.'))
            && !name.chars().any(|c| {
                matches!(
                    c,
                    '*' | '?' | '[' | ']' | '{' | '}' | '!' | '\\' | '(' | ')'
                ) || c.is_control()
            })
    };
    rest.split('/').all(literal).then(|| format!("//{rest}"))
}

/// The path rule form (`//<path>`) of each of a matter folder's `spellings`, for [`within`];
/// a spelling that has no such form (relative, or a root) gives none.
pub(crate) fn matter_roots<'a>(
    spellings: impl IntoIterator<Item = &'a str>,
    windows: bool,
) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for spelling in spellings {
        let root = path(spelling.trim_end_matches(['/', '\\']), windows);
        if let Some(root) = root.filter(|root| !roots.contains(root)) {
            roots.push(root);
        }
    }
    roots
}

/// Whether `rule` stays within its matter, so it can never widen past the data fence: a path
/// rule (`Edit`, `Read`) only for a file strictly inside the matter folder (one of `roots`,
/// [`matter_roots`]) and in neither its `.clauseo` folder nor a `.claude` one (Claude's own
/// settings). So none names `<data>` (Clauseo's data folder, other matters' notes and runs),
/// another matter's folder or anything else outside. Those two names match in any case (a Mac's
/// disk folds case); `windows`: the whole path is compared in any case, each name without its
/// trailing dots and spaces. By text only: [`real_within`] resolves links. Any other rule names
/// no path and passes.
pub(crate) fn within(rule: &str, roots: &[String], windows: bool) -> bool {
    let content = ["Edit(", "Read("]
        .iter()
        .find_map(|tool| rule.strip_prefix(tool))
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(content) = content else {
        return true;
    };
    let fold = |text: &str| {
        if windows {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    let content = fold(content);
    roots.iter().any(|root| {
        let rest = content
            .strip_prefix(&fold(root))
            .and_then(|rest| rest.strip_prefix('/'));
        rest.is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .split('/')
                    .all(|name| !own_folder(&name_as_opened(name, windows)))
        })
    })
}

/// `<notes>/../claude-allow.json`: `<data>/agent-state/<matter>/claude-allow.json`.
pub(crate) fn file_beside(notes: &Path) -> Option<PathBuf> {
    notes.parent().map(|matter| matter.join(FILE))
}

/// `<data>/agent-state/<matter>/claude-allow.json`, as [`file_beside`] the matter's notes.
pub(crate) fn file_of(data: &Path, matter: clauseo_protocol::MatterId) -> PathBuf {
    data.join("agent-state").join(matter.to_string()).join(FILE)
}

/// Whether `requested`, a file a kept path rule names, is inside the matter `folder` by its
/// real path: both resolved through every link and junction (a file not made yet by its folder),
/// the file strictly inside the folder and in neither its `.clauseo` nor a `.claude` folder
/// (any case; on Windows each name without its trailing dots and spaces, as Windows opens it).
/// A link the agent swapped in after the rule was kept is caught here. False when either does
/// not resolve. File system calls: off the actor.
pub(crate) fn real_within(requested: &Path, folder: &Path, windows: bool) -> bool {
    if !requested.is_absolute() {
        return false;
    }
    let Ok(root) = dunce::canonicalize(folder) else {
        return false;
    };
    let real = match dunce::canonicalize(requested) {
        Ok(real) => real,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let (Some(parent), Some(name)) = (requested.parent(), requested.file_name()) else {
                return false;
            };
            match dunce::canonicalize(parent) {
                Ok(parent) => parent.join(name),
                Err(_) => return false,
            }
        }
        Err(_) => return false,
    };
    let names = |path: &Path| -> Vec<String> {
        path.components()
            .map(|part| name_as_opened(&part.as_os_str().to_string_lossy(), windows))
            .collect()
    };
    let (root, real) = (names(&root), names(&real));
    let Some(rest) = real.strip_prefix(root.as_slice()) else {
        return false;
    };
    !rest.is_empty() && rest.iter().all(|name| !own_folder(name))
}

/// A file name as the system opens it, for comparing: on Windows lower case and without its
/// trailing dots and spaces.
fn name_as_opened(name: &str, windows: bool) -> String {
    if windows {
        name.trim_end_matches(['.', ' ']).to_lowercase()
    } else {
        name.to_owned()
    }
}

/// `.clauseo` or `.claude`, in any case.
fn own_folder(name: &str) -> bool {
    [".clauseo", ".claude"]
        .iter()
        .any(|own| own.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests;
