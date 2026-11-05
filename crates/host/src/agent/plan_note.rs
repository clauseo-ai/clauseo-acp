//! Claude's own plan note. In plan mode Claude Code saves its plan as
//! `<config folder>/plans/<name>.md` before it offers the plan exit, outside the matter; in forced
//! ask (`agent/forced_ask.rs`) the pre-write guard asks the lawyer for that write. The host marks
//! such a request `about: agent_plan` so the app can say what it is. The mark is a label only:
//! asking, answering and the guard are the same with or without it.
//!
//! Claude's config folder is the one its launch gives it: `CLAUDE_CONFIG_DIR` in the child's
//! environment when set and not empty, else Claude Code's default `<home>/.claude` (`HOME`, or
//! `USERPROFILE` on Windows). The host sets neither itself: they come from the inherited
//! environment, the agent's settings or the profile hook, merged by `launch_spec`, and
//! [`plans_folder`] reads them from that merged map at the launch. The paths are compared as
//! text split into components, never on the disk, so the actor never waits on a drive.
//!
//! So `about` is a hint: a symbolic link is not followed, and a `plans` folder linked into a
//! matter would still be marked. No reader may skip or soften an ask on it without resolving
//! the path itself.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clauseo_protocol::PermissionAbout;

/// Claude Code's config folder variable.
const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";

/// Claude Code's config folder under the home folder when [`CONFIG_DIR_VAR`] is not set.
const DEFAULT_CONFIG_FOLDER: &str = ".claude";

/// The folder in Claude's config folder that holds its plan notes.
const PLANS_FOLDER: &str = "plans";

/// Claude's plans folder for a launch whose child gets exactly `env`:
/// `<CLAUDE_CONFIG_DIR>/plans` when that is set and not empty, else `<home>/.claude/plans`.
/// Names are matched in any ASCII case when `windows`, as Windows matches them. `None` when
/// neither is set.
pub(crate) fn plans_folder(env: &BTreeMap<OsString, OsString>, windows: bool) -> Option<PathBuf> {
    let var = |name: &str| {
        env.iter()
            .find(|(key, _)| {
                if windows {
                    key.to_string_lossy().eq_ignore_ascii_case(name)
                } else {
                    key.as_os_str() == OsStr::new(name)
                }
            })
            .map(|(_, value)| value.as_os_str())
            .filter(|value| !value.is_empty())
    };
    let config = if let Some(folder) = var(CONFIG_DIR_VAR) {
        PathBuf::from(folder)
    } else {
        let home = var(if windows { "USERPROFILE" } else { "HOME" })?;
        Path::new(home).join(DEFAULT_CONFIG_FOLDER)
    };
    Some(config.join(PLANS_FOLDER))
}

/// What a permission request is about: [`PermissionAbout::AgentPlan`] when `plans` is set (the
/// agent is Claude) and the request names exactly one path, which [`is_plan_note`] in it.
pub(crate) fn about(
    plans: Option<&Path>,
    paths: &[PathBuf],
    windows: bool,
) -> Option<PermissionAbout> {
    match (plans, paths) {
        (Some(plans), [path]) if is_plan_note(plans, path, windows) => {
            Some(PermissionAbout::AgentPlan)
        }
        _ => None,
    }
}

/// Whether `path` is a file directly in `plans` whose name ends in `.md`. Both must be
/// absolute; a `..` in either fails, `.` components are skipped. When `windows`, `/` is a
/// separator too, components match in any case, and a verbatim prefix (`\\?\`, `\\?\UNC\`) is
/// read as the plain spelling, as `dunce` writes it.
pub(crate) fn is_plan_note(plans: &Path, path: &Path, windows: bool) -> bool {
    let (Some(plans), Some(path)) = (components(plans, windows), components(path, windows)) else {
        return false;
    };
    match path.split_last() {
        Some((name, parent)) => parent == plans.as_slice() && is_note_name(name, windows),
        None => false,
    }
}

/// A plan note's name: something before `.md` (`.MD` on Windows, whose components are folded
/// already).
fn is_note_name(name: &str, windows: bool) -> bool {
    name.strip_suffix(if windows { ".MD" } else { ".md" })
        .is_some_and(|stem| !stem.is_empty())
}

/// An absolute path's root then its names, folded by [`fold`] when `windows`; `None` for a relative
/// path, a path holding `..`, or one that is not UTF-8.
fn components(path: &Path, windows: bool) -> Option<Vec<String>> {
    let text = path.to_str()?;
    let (root, rest, separators): (String, String, &[char]) = if windows {
        let (root, rest) = windows_root(text)?;
        (root, rest, &['\\', '/'])
    } else {
        (
            String::from("/"),
            text.strip_prefix('/')?.to_owned(),
            &['/'],
        )
    };
    let mut parts = vec![root];
    for part in rest.split(separators) {
        match part {
            "" | "." => {}
            ".." => return None,
            part if windows => parts.push(fold(part)),
            part => parts.push(part.to_owned()),
        }
    }
    Some(parts)
}

/// A Windows path's root, folded (`C:` or `\\SERVER\SHARE`), and the rest after it; `None`
/// when the path is not absolute (`C:x`, `\x`, `x`) or is a device path (`\\.\`).
fn windows_root(text: &str) -> Option<(String, String)> {
    let text = text.replace('/', "\\");
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text
    };
    if let Some(rest) = text.strip_prefix(r"\\") {
        let mut parts = rest.splitn(3, '\\');
        let server = parts
            .next()
            .filter(|server| !matches!(*server, "" | "." | "?"))?;
        let share = parts.next().filter(|share| !share.is_empty())?;
        let root = fold(&format!(r"\\{server}\{share}"));
        return Some((root, parts.next().unwrap_or_default().to_owned()));
    }
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str().strip_prefix(":\\")?;
    Some((format!("{}:", letter.to_ascii_uppercase()), rest.to_owned()))
}

#[cfg(test)]
mod tests;

/// `text` as Windows compares names: each character upper-cased when that gives exactly one
/// character, else kept as it is, which is closer to NTFS's upcase table than Unicode's
/// lowercase. So the Kelvin sign, the Ohm sign and `İ` stay apart from `k`, `ω` and `i`.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}
