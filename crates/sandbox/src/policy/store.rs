//! Claude's login store: the folder, and the names the jail allows in it.

use std::path::{Path, PathBuf};

use super::{same, under};

/// The files of Claude's login store the jail lets the agent read, write and create, in the
/// folder given to [`PolicyBuilder::claude_store`](super::PolicyBuilder::claude_store): the
/// store itself (the Keychain's fallback on macOS) and the secure-storage write lock's name.
pub const CLAUDE_STORE_FILES: [&str; 2] = [".credentials.json", ".storage-write"];

/// The lock folders Claude makes in that folder (proper-lockfile makes a lock as a folder):
/// the write lock's (`.storage-write` plus `.lock`) and the OAuth refresh lock. Unverified:
/// read from Claude Code 2.1.316's bundled code (`lockfilePath: <dir>/.oauth_refresh.lock`),
/// not seen live; G7 row 17 records the names. See also [`CLAUDE_STORE_BESIDE_LOCK_SUFFIX`].
pub const CLAUDE_STORE_LOCK_FOLDERS: [&str; 2] = [".storage-write.lock", ".oauth_refresh.lock"];

/// The suffix of Claude's legacy OAuth refresh lock, a folder that sits beside the store
/// folder, not in it: `<dir>.lock` (for `~/.claude`, `~/.claude.lock`). Without it a token
/// refresh can fail mid-session. Unverified like the others: read from Claude Code 2.1.316's
/// bundled code (`${realpath(dir)}.lock`), not seen live; G7 row 17 records it.
pub const CLAUDE_STORE_BESIDE_LOCK_SUFFIX: &str = ".lock";

/// The folder holding Claude's login store, and the names the jail allows in it and nothing
/// else (the agent cannot list the folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeStore {
    pub(super) dir: PathBuf,
}

impl ClaudeStore {
    /// The folder, canonical.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// [`CLAUDE_STORE_FILES`] in the folder: read-write literal rules.
    pub fn files(&self) -> Vec<PathBuf> {
        CLAUDE_STORE_FILES
            .iter()
            .map(|name| self.dir.join(name))
            .collect()
    }

    /// The lock folders, read-write subpath rules, each creatable: [`CLAUDE_STORE_LOCK_FOLDERS`]
    /// in the folder, then the legacy lock beside it (`<dir>.lock`,
    /// [`CLAUDE_STORE_BESIDE_LOCK_SUFFIX`]), which sits next to the store folder, not in it.
    pub fn lock_folders(&self) -> Vec<PathBuf> {
        let mut locks: Vec<PathBuf> = CLAUDE_STORE_LOCK_FOLDERS
            .iter()
            .map(|name| self.dir.join(name))
            .collect();
        let mut beside = self.dir.clone().into_os_string();
        beside.push(CLAUDE_STORE_BESIDE_LOCK_SUFFIX);
        locks.push(PathBuf::from(beside));
        locks
    }

    /// Whether `path` is one of the allowed names or under a lock folder.
    pub(super) fn covers(&self, path: &Path) -> bool {
        self.files().iter().any(|file| same(path, file))
            || self.lock_folders().iter().any(|lock| under(path, lock))
    }
}
