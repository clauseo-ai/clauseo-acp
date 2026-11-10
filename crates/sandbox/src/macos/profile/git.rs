//! The profile's git rules that come last (decision record 0038): the launch's probe name, no
//! `.git` or `HEAD` made in the session's temp or notes folder, nothing git reads made at a
//! launch repository's `commondir` target, and the git watch's set-aside folders denied whole.

use std::path::{Path, PathBuf};

use super::names::{GIT_NAME_REGEX, HEAD_REGEX};
use super::params::Params;
use super::{Writer, literal, subpath};
use crate::Policy;

/// The parameter naming the launch's probe: the one Mach name the profile allows looking up
/// that no service registers, by which the git watch finds the session's processes
/// (`jail_procs.rs`).
pub(crate) const SESSION_PROBE: &str = "SESSION_PROBE";

/// What every probe name starts with; the launcher tag, the launch's number and the host
/// process's random part follow (`jail_procs::Probe::new`).
pub(crate) const PROBE_PREFIX: &str = "chat.clauseo.session.";

/// [`SeatbeltProfile::args`] with `probe` for the [`SESSION_PROBE`] parameter: one name for
/// each launch, so each launch's processes are told apart.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn args_with_probe(
    profile: &super::SeatbeltProfile,
    probe: &str,
) -> Vec<std::ffi::OsString> {
    let mut args = profile.args();
    let ours = format!("{SESSION_PROBE}=");
    for arg in &mut args {
        if arg.to_str().is_some_and(|text| text.starts_with(&ours)) {
            *arg = std::ffi::OsString::from(format!("{ours}{probe}"));
        }
    }
    args
}

/// The git rules, after every other file rule.
pub(super) fn rules(w: &mut Writer, policy: &Policy, p: &Params) {
    w.note("\n; The launch's probe name, which no service registers: the lawyer's side finds");
    w.note("; the session's processes by it, whatever group or parent they have.");
    let default = PathBuf::from(format!("{PROBE_PREFIX}{}", w.tag));
    let probe = w.param(SESSION_PROBE, &default);
    w.rule("allow", "mach-lookup", &[format!("(global-name {probe})")]);
    scratch_names(w, p);
    commondir(w, policy);
    set_aside(w, policy);
}

/// No `.git` or `HEAD` made in the session's temp folder or notes: a repository made there
/// moves into the matter under any other name. The state folder is left out: Codex clones
/// its plugin marketplace into `$CODEX_HOME/.tmp/plugins` there.
fn scratch_names(w: &mut Writer, p: &Params) {
    let mut folders = Vec::new();
    if let Some(tmpdir) = &p.tmpdir {
        folders.push(w.param("TMPDIR", tmpdir));
    }
    if let Some(notes) = &p.notes {
        folders.push(w.param("NOTES", notes));
    }
    if folders.is_empty() {
        return;
    }
    w.note("\n; No .git or HEAD in the session's temp folder or notes, in any letter case.");
    let filters: Vec<String> = folders
        .iter()
        .flat_map(|folder| {
            [GIT_NAME_REGEX, HEAD_REGEX]
                .map(|regex| format!("(require-all (subpath {folder}) (regex #\"{regex}\"))"))
        })
        .collect();
    w.rule("deny", "file-write*", &filters);
}

/// Nothing git reads made at a launch repository's `commondir` target outside its rules.
fn commondir(w: &mut Writer, policy: &Policy) {
    let mut filters = Vec::new();
    for (i, target) in policy.git.commondir.iter().enumerate() {
        for (j, path) in target.denied().iter().enumerate() {
            let param = w.param(&format!("COMMONDIR_{i}_{j}"), path);
            filters.push(literal(&param));
            filters.push(subpath(&param));
        }
    }
    if filters.is_empty() {
        return;
    }
    w.note("\n; Launch repositories' commondir targets: nothing git reads made there.");
    w.rule("deny", "file-write*", &filters);
}

/// The git watch's set-aside folders, last of the file rules: the data dir's, inside a
/// hidden path already (the policy checked), and the matter volume's `.clauseo` folder.
fn set_aside(w: &mut Writer, policy: &Policy) {
    let dirs: Vec<(&str, &Path)> = [
        ("GIT_SET_ASIDE", policy.git_set_aside()),
        ("VOLUME_SET_ASIDE", policy.volume_set_aside()),
    ]
    .into_iter()
    .filter_map(|(name, dir)| Some((name, dir?)))
    .collect();
    if dirs.is_empty() {
        return;
    }
    w.note("\n; The git watch's set-aside folders: never the agent's, whatever came above.");
    let filters: Vec<String> = dirs
        .into_iter()
        .map(|(name, dir)| subpath(&w.param(name, dir)))
        .collect();
    w.rule("deny", "file-read* file-write*", &filters);
}
