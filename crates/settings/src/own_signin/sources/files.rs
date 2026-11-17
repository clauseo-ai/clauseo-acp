//! Where Claude Code's settings sources are on this machine, and reading one file of them.

use std::fs::File;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};

use super::MAX_FILE;

/// Claude Code's managed-preferences domain.
#[cfg(any(target_os = "macos", test))]
const DOMAIN: &str = "com.anthropic.claudecode";

/// The most `managed-settings.d` documents read; more make the read unsure.
const MAX_DROP_INS: usize = 64;

/// The folder of Claude Code's `managed-settings.json` and `managed-mcp.json` on this OS.
pub fn claude_managed_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Library/Application Support/ClaudeCode");
    #[cfg(windows)]
    return PathBuf::from(r"C:\Program Files\ClaudeCode");
    #[cfg(not(any(windows, target_os = "macos")))]
    return PathBuf::from("/etc/claude-code");
}

/// Claude Code's policy files in `managed`: `managed-settings.json`, then the drop-ins; `Err`
/// when the drop-ins cannot be listed or are too many.
pub(super) fn managed_files(managed: &Path) -> Result<Vec<PathBuf>, ()> {
    let mut files = vec![managed.join("managed-settings.json")];
    let folder = managed.join("managed-settings.d");
    let entries = match std::fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) if absent(error.kind()) => return Ok(files),
        Err(_) => return Err(()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.map_err(drop)?.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".json") && !name.starts_with('.') {
            names.push(name.into_owned());
        }
        if names.len() > MAX_DROP_INS {
            return Err(());
        }
    }
    names.sort();
    files.extend(names.into_iter().map(|name| folder.join(name)));
    Ok(files)
}

/// The managed-preferences files Claude Code reads on this machine: none but on macOS.
pub(super) fn plist_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let user = crate::policy::managed_prefs::login_user().ok().flatten();
        plist_paths_for(user.as_deref())
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// The managed-preferences files for the login user `user` in Claude Code's order: the user's,
/// then the computer's (the computer's alone without a user, as Claude Code skips the user's
/// file when it finds no name). `file_paths` lists the computer's first, for its other callers.
#[cfg(any(target_os = "macos", test))]
pub(super) fn plist_paths_for(user: Option<&str>) -> Vec<PathBuf> {
    use crate::policy::managed_prefs::{MANAGED_DIR, file_paths};
    let mut paths = user
        .and_then(|user| file_paths(DOMAIN, user).ok())
        .unwrap_or_else(|| vec![Path::new(MANAGED_DIR).join(format!("{DOMAIN}.plist"))]);
    paths.reverse();
    paths
}

/// Whether `value` holds a date or data value at any depth, which plutil cannot turn into JSON.
pub(super) fn holds_date_or_data(value: &plist::Value) -> bool {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            plist::Value::Date(_) | plist::Value::Data(_) => return true,
            plist::Value::Array(items) => stack.extend(items),
            plist::Value::Dictionary(entries) => stack.extend(entries.values()),
            _ => {}
        }
    }
    false
}

/// `path`'s bytes: `None` when it is not there, `Err` when it is there but cannot be read or
/// holds more than [`MAX_FILE`].
pub(super) fn read_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if absent(error.kind()) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(drop)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_FILE) {
        return Err(());
    }
    Ok(Some(bytes))
}

/// Whether an open's error says the file is not there (or its folder is not a folder).
fn absent(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// `bytes` without a leading UTF-8 byte order mark.
pub(super) fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}
