//! The run's Chrome profile before each start: kept or made, with the preferences the guard
//! needs (`chrome.rs`). Every file the guard reads or writes in a kept profile is opened without
//! following a link at its name, and every folder it writes into must be a real folder: a
//! symlink, junction or other reparse point there could otherwise send the read, or the write,
//! anywhere the guard's user may write. Each file is written to a new temporary name in the same
//! folder and then renamed over the old name, which replaces a link there instead of following it.
//!
//! On Unix the folder is opened once, with `O_NOFOLLOW`, and the temporary file and the rename
//! are relative to that open folder (`write::replace_in_folder`): a link swapped in for
//! `Default` after its check fails the open, and is never written through. On Windows the
//! temporary file and the rename resolve the folder's path again, so a junction swapped in for
//! `Default` between the check and the write would redirect it. That window is bounded by the
//! jail: the guard runs inside the agent's jail with the agent's own rights (decision record 0029,
//! "Chrome on the claimed port"), so a write through a swapped link reaches nothing the agent
//! could not write itself.

use std::io;
use std::path::Path;

use super::super::{logon, quiet};
use crate::write;

/// Keeps `profile` when an earlier start of this run made it (a real folder, not a link), with
/// its staging folder, and merges [`quiet::preferences`] and [`logon::preferences`] into the
/// `Default/Preferences` Chrome rewrote, and [`logon::ready_local_state`] into its `Local
/// State`, so every start of the run gets them; else makes it from nothing, with
/// `Default/Preferences` sending downloads to `staging` and those services off, and
/// [`logon::local_state`] as `Local State`, so Chrome makes no failed Windows logon. `staging`
/// is a folder directly in `profile`. A link at either file's name is never followed: its target
/// is not read, and the write replaces the link ([`write_new`]; on Windows within the jail's
/// bound, see the top of this file).
///
/// agent-browser's launch let every download through (`Browser.setDownloadBehavior` `allow`);
/// connected to the guard's Chrome it sets `default` (`browser.rs:2563-2574`), so Chrome's own
/// rules apply. One of them holds a download over `http://` of a "high-risk" type (`.md`,
/// `.pdf`, ...) as `*.crdownload` until someone clicks Keep, which never happens headless.
/// Chrome waives that rule where the page's "insecure content" setting is allow, so the profile
/// allows it for `http://` pages only: there no content is mixed, so nothing else changes. A
/// download over `http://` from an `https://` page stays blocked, as in Chrome by default.
pub(in super::super) fn ready_profile(
    profile: &Path,
    staging: &Path,
    downloads: &Path,
) -> io::Result<()> {
    let default = profile.join("Default");
    let file = default.join("Preferences");
    let local_state = profile.join("Local State");
    match std::fs::symlink_metadata(profile) {
        Ok(found) if found.is_dir() => {
            real_folder(staging).map_err(|error| step("make", profile, staging, &error))?;
            real_folder(&default).map_err(|error| step("make", profile, &default, &error))?;
            // Chrome's own rewrite, or ours again if it is gone, unreadable or a link.
            let mut preferences = read_json(&file)
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| first_preferences(staging, downloads));
            merge(&mut preferences, quiet::preferences());
            merge(&mut preferences, logon::preferences());
            let kept = read_json(&local_state);
            write_new(
                profile,
                &local_state,
                logon::ready_local_state(kept).to_string(),
            )?;
            return write_new(profile, &file, preferences.to_string());
        }
        Ok(_) => write::remove_dir_all(profile)
            .or_else(|_| write::remove_file(profile))
            .map_err(|error| step("remove the non-folder at", profile, profile, &error))?,
        Err(_) => {}
    }
    write::create_dir_all(profile).map_err(|error| step("make", profile, profile, &error))?;
    real_folder(&default).map_err(|error| step("make", profile, &default, &error))?;
    real_folder(staging).map_err(|error| step("make", profile, staging, &error))?;
    write_new(profile, &local_state, logon::local_state().to_string())?;
    write_new(
        profile,
        &file,
        first_preferences(staging, downloads).to_string(),
    )
}

/// A new profile's `Default/Preferences`: downloads to `staging`, the mixed-content rule above,
/// [`quiet::preferences`] and [`logon::preferences`].
fn first_preferences(staging: &Path, downloads: &Path) -> serde_json::Value {
    let mut preferences = serde_json::json!({
        "download": {
            "default_directory": staging,
            "prompt_for_download": false,
            "directory_upgrade": true,
        },
        "savefile": {"default_directory": downloads},
        "profile": {"content_settings": {"exceptions": {"mixed_script": {
            "http://*:*,*": {"setting": 1},
        }}}},
    });
    merge(&mut preferences, quiet::preferences());
    merge(&mut preferences, logon::preferences());
    preferences
}

/// Puts `from`'s entries into `into`, object by object: what `into` has beside them stays.
fn merge(into: &mut serde_json::Value, from: serde_json::Value) {
    match (into, from) {
        (serde_json::Value::Object(into), serde_json::Value::Object(from)) => {
            for (key, value) in from {
                merge(into.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
        (into, from) => *into = from,
    }
}

/// `file`'s JSON; `None` when it is missing, unreadable, not JSON, or not a plain file (a link
/// is not followed: `write::read_no_follow`).
fn read_json(file: &Path) -> Option<serde_json::Value> {
    let bytes = write::read_no_follow(file).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Makes `path`, in a folder that exists, a real folder: `create_dir_all` would accept a link
/// to a folder, and each write into it would land in the link's target. A link or file there is
/// removed first (a junction on Windows by `remove_dir`, which removes the link, never what it
/// points at); fails when something that is not a real folder stays.
fn real_folder(path: &Path) -> io::Result<()> {
    let is_real = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|found| found.is_dir());
    if is_real(path) {
        return Ok(());
    }
    if std::fs::symlink_metadata(path).is_ok() {
        // Whichever of the two fits the link; the other fails, and `create_dir` then says why.
        drop(write::remove_file(path).or_else(|_| write::remove_dir(path)));
    }
    write::create_dir(path)?;
    if is_real(path) {
        Ok(())
    } else {
        // No path in the text: `step` names the folder relative to the profile.
        Err(io::Error::other("not a real folder"))
    }
}

/// Writes `contents` to `file`, in `profile`, without following a link at its name
/// (`write::replace_beside`: a new temporary file renamed over it; on Unix relative to the folder
/// opened once with `O_NOFOLLOW`). The error names the file relative to `profile`.
fn write_new(profile: &Path, file: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    write::replace_beside(file, contents.as_ref())
        .map_err(|error| step("write", profile, file, &error))
}

/// `error` with the step and what it was for, so a failed start says which: "could not write
/// Default/Preferences: <error>". The error reaches the agent (the start's refusal), so it names
/// `path` only relative to `profile` ([`named`]), never with the folders above it, which can
/// hold the user's name. The errors wrapped here hold no path of their own (`real_folder`'s, and
/// `io::Error`s from the OS, carry none).
fn step(what: &str, profile: &Path, path: &Path, error: &io::Error) -> io::Error {
    let shown = named(profile, path);
    io::Error::new(error.kind(), format!("could not {what} {shown}: {error}"))
}

/// `path` as an error shows it: relative to `profile` (`Default/Preferences`, `Default`), "the
/// profile folder" for `profile` itself, and "a folder outside the profile" for anything else
/// (no caller passes one).
fn named(profile: &Path, path: &Path) -> String {
    match path.strip_prefix(profile) {
        Ok(rest) if rest.as_os_str().is_empty() => "the profile folder".to_owned(),
        Ok(rest) => rest
            .iter()
            .map(|name| name.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => "a folder outside the profile".to_owned(),
    }
}

#[cfg(test)]
mod tests;
