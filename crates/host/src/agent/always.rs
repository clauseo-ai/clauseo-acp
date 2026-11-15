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
//!   the tool's name and `rawInput` alone: `Bash(<exact command>)`, `WebFetch(domain:<host>)`,
//!   an MCP tool's exact name, `Edit(//<path>)` for `Edit`, `Write` and `MultiEdit`,
//!   `Read(//<path>)`. Anything else, and any text that would not stay one exact rule (a `)`,
//!   a glob character, a relative path), gives none: the request is still answered once, and
//!   nothing is kept.
//! - [`file_beside`]: `<data>/agent-state/<matter>/claude-allow.json`, beside the matter's notes
//!   and not in them: Claude may write the notes folder (`additionalDirectories`), while this
//!   file is in the data fence's fixed deny rules (`data_fence::DENIED`) and outside every folder
//!   the auto-deny counts as the session's own. Nothing is ever written in the matter folder,
//!   and a `.claude` folder the lawyer made there is left as it is.
//! - [`read`] at each Claude launch (off the actor), [`store`] on its own thread, both under one
//!   lock so two answers never lose a rule.
//! - [`allow_in_meta`]: a launch outside forced ask adds the kept rules to
//!   `permissions.allow` in the flag settings (`_meta.claudeCode.options.settings`); Claude Code
//!   still takes a matching deny rule first.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value};

/// The file's name, in `<data>/agent-state/<matter>`.
pub(crate) const FILE: &str = "claude-allow.json";

/// The most rules a matter keeps; an answer past it keeps nothing more.
pub(crate) const MAX_RULES: usize = 500;

/// The longest rule kept, in bytes.
const MAX_RULE_LEN: usize = 2_000;

/// The largest file [`read`] takes.
const MAX_FILE_LEN: u64 = 1 << 20;

/// The one lock of every read and write of a rules file.
static LOCK: Mutex<()> = Mutex::new(());

/// The rule a request keeps, from its tool's `name` (the tool call's, as the agent sent it) and
/// its `rawInput`, or none (see the module doc). `windows`: a file path is a Windows one.
pub(crate) fn rule(tool: &str, raw_input: Option<&Value>, windows: bool) -> Option<String> {
    let text = |key: &str| raw_input?.get(key)?.as_str();
    let rule = match tool {
        "Bash" => format!("Bash({})", command(text("command")?)?),
        "WebFetch" => format!("WebFetch(domain:{})", domain(text("url")?)?),
        "Edit" | "Write" | "MultiEdit" => format!("Edit({})", path(text("file_path")?, windows)?),
        "Read" => format!("Read({})", path(text("file_path")?, windows)?),
        name if is_mcp_tool(name) => name.to_owned(),
        _ => return None,
    };
    (rule.len() <= MAX_RULE_LEN).then_some(rule)
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
/// `{`, `}`, `!`, `\`), a parenthesis, a control character, or a space at either end.
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
/// disk folds case); `windows`: the whole path is compared in any case. Any
/// other rule names no path and passes; a kept command still runs in the sandbox, which has no
/// way out (`loads::sandbox`, `allowUnsandboxedCommands: false`).
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
                && rest.split('/').all(|name| {
                    ![".clauseo", ".claude"]
                        .iter()
                        .any(|own| own.eq_ignore_ascii_case(name))
                })
        })
    })
}

/// `<notes>/../claude-allow.json`: `<data>/agent-state/<matter>/claude-allow.json`.
pub(crate) fn file_beside(notes: &Path) -> Option<PathBuf> {
    notes.parent().map(|matter| matter.join(FILE))
}

/// A Claude launch's kept rules (`claude`), read from [`file_beside`] `notes`; none for any
/// other agent. File system calls: off the actor.
pub(crate) fn read_beside(notes: &Path, claude: bool) -> Vec<String> {
    match file_beside(notes) {
        Some(file) if claude => read(&file),
        _ => Vec::new(),
    }
}

/// The rules kept in `file`, each [`well_formed`], at most [`MAX_RULES`]; none when it is
/// missing, a link, too large or unreadable. File system calls: off the actor.
pub(crate) fn read(file: &Path) -> Vec<String> {
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    read_unlocked(file)
}

fn read_unlocked(file: &Path) -> Vec<String> {
    let Ok(opened) = crate::write::open_read_nofollow(file) else {
        return Vec::new();
    };
    let mut text = String::new();
    if opened.take(MAX_FILE_LEN).read_to_string(&mut text).is_err() {
        tracing::warn!("a matter's kept Claude rules are unreadable");
        return Vec::new();
    }
    let parsed: Option<Vec<String>> = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| serde_json::from_value(value.get("allow")?.clone()).ok());
    let Some(rules) = parsed else {
        tracing::warn!("a matter's kept Claude rules are not JSON this host reads");
        return Vec::new();
    };
    let mut kept: Vec<String> = Vec::new();
    for rule in rules {
        if well_formed(&rule) && !kept.contains(&rule) && kept.len() < MAX_RULES {
            kept.push(rule);
        }
    }
    kept
}

/// Adds `rule` to `file` on a thread of its own: read, merged, then replaced atomically through
/// no link. A failure is a warning that names no rule (a command may hold a client's name).
pub(crate) fn store(file: PathBuf, rule: String) {
    let spawned = std::thread::Builder::new()
        .name("clauseo-always".to_owned())
        .spawn(move || {
            if let Err(error) = store_now(&file, rule) {
                tracing::warn!(%error, "a matter's Claude rule was not kept");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a matter's Claude rule was not kept");
    }
}

/// [`store`]'s work, on the calling thread.
pub(crate) fn store_now(file: &Path, rule: String) -> std::io::Result<()> {
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rules = read_unlocked(file);
    if rules.contains(&rule) || rules.len() >= MAX_RULES || !well_formed(&rule) {
        return Ok(());
    }
    rules.push(rule);
    let text = serde_json::json!({ "version": 1, "allow": rules });
    let mut bytes = serde_json::to_vec_pretty(&text).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    crate::write::replace_private_file(file, &bytes)?;
    match file.parent() {
        Some(folder) => crate::write::sync_dir(folder),
        None => Ok(()),
    }
}

/// Adds `rules` to `claudeCode.options.settings.permissions.allow` in a Claude session's
/// `_meta`, after any already there and without repeats. A value on the way that is not an
/// object (or, at `allow`, not an array) is replaced.
pub(crate) fn allow_in_meta<'a>(
    meta: &mut Map<String, Value>,
    rules: impl IntoIterator<Item = &'a str>,
) {
    let mut rules = rules.into_iter().peekable();
    if rules.peek().is_none() {
        return;
    }
    let mut at = meta;
    for key in ["claudeCode", "options", "settings", "permissions"] {
        let slot = at
            .entry(key.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Value::Object(next) = slot else {
            return;
        };
        at = next;
    }
    let allow = at
        .entry("allow".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !allow.is_array() {
        *allow = Value::Array(Vec::new());
    }
    let Value::Array(allow) = allow else {
        return;
    };
    for rule in rules {
        if !allow.iter().any(|item| item.as_str() == Some(rule)) {
            allow.push(Value::from(rule));
        }
    }
}

#[cfg(test)]
mod tests;
