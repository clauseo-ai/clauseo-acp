//! Setting a rejected file aside before the first write would replace it: the file at `path`
//! moves to `<its name>.rejected-<UTC time>` beside it (`allowed.json`'s integrity check,
//! `blocklist/allowed/integrity.rs`, and the sandbox's owed forgets,
//! `actor/protection/forget/threads.rs`), never replacing a file, and only the latest few are
//! kept. The temp-file sweep (`.clauseo-*.new`) never takes them.

use std::io;
use std::path::Path;

use clauseo_protocol::Timestamp;

use crate::log_file::Cause;

/// The set-aside files' name before their time: `<name>.rejected-`.
fn prefix(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{name}.rejected-")
}

/// Moves the file at `path` to `<name>.rejected-<UTC now, YYYYMMDDTHHMMSSZ>` beside it, with
/// `-<n>` after a name already taken, never replacing a file. Returns the new file name.
pub(crate) fn set_aside(path: &Path, now: Timestamp) -> io::Result<String> {
    let folder = path.parent().unwrap_or(path);
    let stem = format!("{}{}", prefix(path), utc_stamp(now));
    for n in 0..100_u32 {
        let name = if n == 0 {
            stem.clone()
        } else {
            format!("{stem}-{n}")
        };
        match crate::write::move_file_no_replace(path, &folder.join(&name)) {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every set-aside name is taken",
    ))
}

/// Removes all but the latest `keep` set-aside files beside `path` (by name, which starts with
/// the time). One that does not go is logged and left.
pub(crate) fn prune(path: &Path, keep: usize) {
    let folder = path.parent().unwrap_or(path);
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let prefix = prefix(path);
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    names.sort_by_key(|name| sort_key(&prefix, name));
    let old = names.len().saturating_sub(keep);
    for name in names.iter().take(old) {
        if let Err(error) = crate::write::remove_old_file(&folder.join(name)) {
            tracing::warn!(name, error = %Cause(&error), "an old set-aside file was kept");
        }
    }
}

/// A set-aside name's order: its time, then its counter as a number.
fn sort_key(prefix: &str, name: &str) -> (String, u32) {
    let rest = name.trim_start_matches(prefix);
    match rest.split_once('-') {
        Some((time, n)) => (time.to_owned(), n.parse().unwrap_or(0)),
        None => (rest.to_owned(), 0),
    }
}

/// `now` as `YYYYMMDDTHHMMSSZ`, in UTC.
fn utc_stamp(now: Timestamp) -> String {
    let secs = now.as_millis().div_euclid(1000);
    let (year, month, day) = crate::connectors::civil_from_days(secs.div_euclid(86_400));
    let time = secs.rem_euclid(86_400);
    let (hour, minute, second) = (time / 3600, time % 3600 / 60, time % 60);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}
