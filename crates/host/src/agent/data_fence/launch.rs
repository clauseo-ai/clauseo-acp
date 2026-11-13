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

use super::{DENIED, READ_ONLY, SHARED, escape};

/// Stands for any one name in an own folder's place ([`claude_projects`]'s session id). A
/// file name can't hold a NUL, so no real name is this. Followed by text, it stands for any name
/// that starts with the text ([`READ_ONLY`]'s `clauseo-python-*`).
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
    /// What the launch left out of [`Self::targets`], as patterns in the same form (a `*` is
    /// any name, or any rest of one): the session's own folders, [`SHARED`], [`READ_ONLY`]
    /// and the own project's `tool-results` folders. The shell rules
    /// (`super::windows_shell_deny_rules`) end a target's name at a name boundary when one of
    /// these could start with it.
    pub(crate) kept: Vec<String>,
}

impl LaunchDeny {
    /// The rules for `spellings` of `<data>` before the launch prep enumerates it: [`DENIED`].
    pub(crate) fn fixed(spellings: Vec<PathBuf>) -> Self {
        let targets = DENIED.iter().map(|target| (*target).to_owned()).collect();
        let shared = SHARED.iter().map(|(place, _)| escape(place));
        let kept = shared
            .chain(READ_ONLY.iter().map(|place| (*place).to_owned()))
            .collect();
        Self {
            spellings,
            targets,
            kept,
        }
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
    let places = places(data, own, projects);
    for target in existing_targets(data, &places) {
        if !deny.targets.contains(&target) {
            deny.targets.push(target);
        }
    }
    deny.kept = places
        .own
        .iter()
        .chain(&places.open)
        .map(|names| {
            let names: Vec<String> = names
                .iter()
                .map(|name| match name.strip_prefix(ANY_NAME) {
                    Some(start) => format!("{}*", escape(start)),
                    None => escape(name),
                })
                .collect();
            names.join("/")
        })
        .collect();
    deny
}

/// The patterns that deny every entry of `data` that exists now, except the `own` places of
/// `places` ([`places`]: the session's own folders, the [`SHARED`] ones, each
/// `<project>/<any name>/tool-results`), each real folder that fits one of its `open` places
/// ([`READ_ONLY`]; a link that fits is denied), and the folders above them,
/// which are walked into instead: a folder is `<name>/**`, a link both `<name>` and
/// `<name>/**`, anything else `<name>`, names glob-escaped. A link is never walked into. A
/// name that is not UTF-8 gets no pattern (no rule could name it); the auto-deny still covers
/// it.
fn existing_targets(data: &Path, places: &Places) -> Vec<String> {
    let mut out = Vec::new();
    walk(data, &mut Vec::new(), places, &mut out);
    out
}

/// The places [`existing_targets`] leaves out, as names under `data`: `own`, [`SHARED`], each
/// `<project>/<any name>/tool-results` of `projects` and [`READ_ONLY`].
fn places(data: &Path, own: &[PathBuf], projects: &[PathBuf]) -> Places {
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
    let open: Vec<Vec<String>> = READ_ONLY.iter().map(|place| pattern_place(place)).collect();
    Places { own, open }
}

/// The places [`walk`] leaves out: `own`, whatever kind of entry is there, and `open`, only
/// where a real folder is (each as names, [`fits`]).
struct Places {
    own: Vec<Vec<String>>,
    open: Vec<Vec<String>>,
}

/// A [`READ_ONLY`] pattern as names for [`fits`]: a final `*` in a name is [`ANY_NAME`]
/// followed by the name's start.
fn pattern_place(pattern: &str) -> Vec<String> {
    pattern
        .split('/')
        .map(|name| match name.strip_suffix('*') {
            Some(start) => format!("{ANY_NAME}{start}"),
            None => name.to_owned(),
        })
        .collect()
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

/// The folders whose entries grow with the matters (run folders, matters' state, Claude's
/// projects), whose other entries [`walk`] names in groups ([`group_prefix`]).
const GROUPED: [&[&str]; 3] = [
    &["run"],
    &["agent-state"],
    &["agents", "claude-acp", "home", "projects"],
];

/// [`existing_targets`] for the folder `dir`, at `at` (its names under `<data>`). In a
/// [`GROUPED`] folder, each entry that is not kept and whose name no kept name there starts
/// with is named by its group ([`group_prefix`]): `<prefix>*` and `<prefix>*/**`, once for
/// all the entries that share it. Such a pattern names no kept entry, and a name made there
/// after the launch with the same start too; a busy data folder's thousands of entries there
/// become a few dozen patterns (the settings file Claude Code reads is at most 2 MiB,
/// `agent::launch_settings`). An entry whose name a kept name starts with is named whole.
fn walk(dir: &Path, at: &mut Vec<String>, places: &Places, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let kept_here = GROUPED
        .iter()
        .any(|folder| folder.iter().copied().eq(at.iter().map(String::as_str)))
        .then(|| kept_names(places, at))
        .flatten();
    let mut groups = std::collections::BTreeSet::new();
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        at.push(name);
        let real_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let above = |folder: &Vec<String>| fits(folder.get(..at.len()).unwrap_or_default(), at);
        if places.own.iter().any(|folder| fits(folder, at)) {
            // The session's own.
        } else if real_dir && places.open.iter().any(|folder| fits(folder, at)) {
            // Every session's to read and run (`READ_ONLY`).
        } else if real_dir && places.own.iter().chain(&places.open).any(above) {
            walk(&entry.path(), at, places, out);
        } else if let Some(group) = kept_here
            .as_ref()
            .and_then(|kept| group_prefix(at.last().map_or("", String::as_str), kept))
        {
            groups.insert(group);
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
    let folder: Vec<String> = at.iter().map(|name| escape(name)).collect();
    let folder = folder.join("/");
    for group in groups {
        let pattern = format!("{folder}/{}*", escape(&group));
        out.push(pattern.clone());
        out.push(format!("{pattern}/**"));
    }
}

/// The names the kept places have in the folder `at`, or none when one of them is a pattern
/// (any name there may be kept, so nothing is grouped).
fn kept_names(places: &Places, at: &[String]) -> Option<Vec<String>> {
    let mut kept = Vec::new();
    for place in places.own.iter().chain(&places.open) {
        let Some((name, above)) = place.get(..=at.len()).and_then(<[String]>::split_last) else {
            continue;
        };
        if fits(above, at) {
            if name.starts_with(ANY_NAME) {
                return None;
            }
            kept.push(name.clone());
        }
    }
    Some(kept)
}

/// The start of `name` that no name of `kept` starts with, compared in any case (as Windows
/// names are): one character past the longest start it shares with one of them; none when a
/// kept name starts with all of `name` (the run `…-1` beside the own `…-10`).
fn group_prefix(name: &str, kept: &[String]) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let same = |a: &char, b: &char| a.to_lowercase().eq(b.to_lowercase());
    let shared = kept
        .iter()
        .map(|kept| {
            let kept: Vec<char> = kept.chars().collect();
            kept.iter()
                .zip(&chars)
                .take_while(|(a, b)| same(a, b))
                .count()
        })
        .max()
        .unwrap_or(0);
    chars.get(..=shared).map(|start| start.iter().collect())
}

/// Whether the own place `folder` names the place `at`: as long, each name equal, or
/// [`ANY_NAME`] followed by a start of the name (nothing: any name).
fn fits(folder: &[String], at: &[String]) -> bool {
    folder.len() == at.len()
        && folder.iter().zip(at).all(|(own, name)| {
            own == name
                || own
                    .strip_prefix(ANY_NAME)
                    .is_some_and(|start| name.starts_with(start) && name.len() > start.len())
        })
}
