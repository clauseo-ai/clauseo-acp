//! Claude's deny rules for one launch: [`DENIED`] (what may not exist yet), then every entry of
//! `<data>` that exists at the launch and is not the session's own, found on the launch-prep
//! thread. Claude Code has no allow rule that beats a deny, so the session's own folders (its
//! run folder, this matter's notes, Claude's plans folder, and the matter folder when it sits in
//! `<data>`) are left out by walking down to them: every sibling on the way is denied.
//!
//! Claude's transcripts live in its home's `projects/<project key>` folders, and so do the
//! large tool results it saves and has the model Read back (`<session id>/tool-results`, which
//! Claude Code reads unasked only when no deny rule covers it). So every transcript, subagent
//! transcript and memory folder there is denied by pattern ([`DENIED`]), every other project's
//! folder that exists is denied whole, and in this session's own project only its sessions'
//! `tool-results` folders are left out ([`claude_projects`]).
//!
//! The residual, accepted: a run folder, a matter's notes or another matter's Claude project
//! made after the launch is in no rule until the next launch (its transcripts are). The host's
//! auto-deny ([`super::FenceCheck`]) refuses a request for any of them when Claude asks, but
//! Claude does not ask for every read (decision record 0030).

use std::path::{Component, Path, PathBuf};

use super::{DENIED, SHARED, escape};

/// Stands for any one name in an own folder's place ([`claude_projects`]'s session id). A
/// file name can't hold a NUL, so no real name is this.
const ANY_NAME: &str = "\0";

/// The longest project key Claude Code uses whole; a longer one is cut and given a hash.
const MAX_KEY: usize = 200;

/// Claude Code's project key for a working folder `cwd`, the name of the folder in its home's
/// `projects` that holds the transcripts and tool results of sessions there (Claude Code
/// 0.3.293's `kI`): each UTF-16 unit that is not an ASCII letter or digit made `-`; when that
/// is longer than 200, its first 200 and `-` and the base-36 hash of `cwd` (the absolute value
/// of the 32-bit `h = 31 h + unit` over its UTF-16 units).
pub(crate) fn claude_project_key(cwd: &str) -> String {
    let units: Vec<u16> = cwd.encode_utf16().collect();
    let key: String = units
        .iter()
        .map(|unit| match char::from_u32(u32::from(*unit)) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if key.len() <= MAX_KEY {
        return key;
    }
    let hash = units.iter().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(*unit))
    });
    let mut left = u64::from(hash.unsigned_abs());
    let mut digits = Vec::new();
    loop {
        let digit = u32::try_from(left % 36).unwrap_or_default();
        digits.push(char::from_digit(digit, 36).unwrap_or('0'));
        left /= 36;
        if left == 0 {
            break;
        }
    }
    let hash: String = digits.into_iter().rev().collect();
    format!("{}-{hash}", key.get(..MAX_KEY).unwrap_or_default())
}

/// The folders in Claude's `home` that hold this session's transcripts and tool results: its
/// `projects/<key>` for each spelling of the working folder in `cwds` (none that is not UTF-8).
pub(crate) fn claude_projects(home: &Path, cwds: &[&Path]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for cwd in cwds.iter().filter_map(|cwd| cwd.to_str()) {
        let folder = home.join("projects").join(claude_project_key(cwd));
        if !out.contains(&folder) {
            out.push(folder);
        }
    }
    out
}

/// Claude's data folder deny rules for one launch: the spellings of `<data>` and the patterns
/// under it, each denied for `Read` and `Edit` ([`super::claude_deny_rules`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LaunchDeny {
    /// `<data>`'s canonical path, then the path as configured when it differs.
    pub(crate) spellings: Vec<PathBuf>,
    /// Rule patterns under `<data>`, `/`-separated: [`DENIED`], then [`existing_targets`].
    pub(crate) targets: Vec<String>,
}

impl LaunchDeny {
    /// The rules for `spellings` of `<data>` before the launch prep enumerates it: [`DENIED`].
    pub(crate) fn fixed(spellings: Vec<PathBuf>) -> Self {
        let targets = DENIED.iter().map(|target| (*target).to_owned()).collect();
        Self { spellings, targets }
    }
}

/// The deny rules of a launch with `spellings` of `<data>` at `data`, leaving out `own`, the
/// session's own folders, and the `tool-results` folder of every session in `projects`, its
/// Claude project folders ([`claude_projects`]); each as a path under `data` (one elsewhere
/// is ignored).
pub(crate) fn launch_deny(
    spellings: Vec<PathBuf>,
    data: &Path,
    own: &[PathBuf],
    projects: &[PathBuf],
) -> LaunchDeny {
    let mut deny = LaunchDeny::fixed(spellings);
    for target in existing_targets(data, own, projects) {
        if !deny.targets.contains(&target) {
            deny.targets.push(target);
        }
    }
    deny
}

/// The patterns that deny every entry of `data` that exists now, except the `own` folders, the
/// [`SHARED`] ones, each `<project>/<any name>/tool-results` of `projects`, and the folders
/// above them, which are walked into instead: a folder is `<name>/**`, a link both `<name>` and
/// `<name>/**`, anything else `<name>`, names glob-escaped. A link is never walked into. A
/// name that is not UTF-8 gets no pattern (no rule could name it); the auto-deny still covers
/// it.
pub(crate) fn existing_targets(data: &Path, own: &[PathBuf], projects: &[PathBuf]) -> Vec<String> {
    let place = |folder: &PathBuf| folder.strip_prefix(data).ok().and_then(names);
    let mut own: Vec<Vec<String>> = own
        .iter()
        .filter_map(place)
        .filter(|names| !names.is_empty())
        .collect();
    own.extend(SHARED.iter().map(|(place, _)| vec![(*place).to_owned()]));
    for mut project in projects.iter().filter_map(place) {
        project.extend([ANY_NAME.to_owned(), "tool-results".to_owned()]);
        own.push(project);
    }
    let mut out = Vec::new();
    walk(data, &mut Vec::new(), &own, &mut out);
    out
}

/// `place`'s names, or `None` when it holds anything but names or a name that is not UTF-8.
fn names(place: &Path) -> Option<Vec<String>> {
    place
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str().map(str::to_owned),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect()
}

/// [`existing_targets`] for the folder `dir`, at `at` (its names under `<data>`).
fn walk(dir: &Path, at: &mut Vec<String>, own: &[Vec<String>], out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        at.push(name);
        let real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if own.iter().any(|folder| fits(folder, at)) {
            // The session's own.
        } else if real_dir
            && own
                .iter()
                .any(|folder| fits(folder.get(..at.len()).unwrap_or_default(), at))
        {
            walk(&entry.path(), at, own, out);
        } else {
            let pattern = at.iter().map(|name| escape(name)).collect::<Vec<_>>();
            let pattern = pattern.join("/");
            let link = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            if real_dir || link {
                out.push(format!("{pattern}/**"));
            }
            if !real_dir {
                out.push(pattern);
            }
        }
        at.pop();
    }
}

/// Whether the own place `folder` names the place `at`: as long, each name equal or
/// [`ANY_NAME`].
fn fits(folder: &[String], at: &[String]) -> bool {
    folder.len() == at.len()
        && folder
            .iter()
            .zip(at)
            .all(|(own, name)| own == ANY_NAME || own == name)
}
