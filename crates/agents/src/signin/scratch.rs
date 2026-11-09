//! Claude detection's own config folder: `CLAUDE_CONFIG_DIR` for its one `auth status` run
//! (`RunKind::Detect`), so Claude Code writes nothing into the lawyer's `~/.claude` or
//! `~/.claude.json`.
//!
//! Why: the real CLI's `auth status` rewrites its config file and writes
//! `backups/.claude.json.backup.<ms>` the first time a CLI version meets a config folder, and
//! creates both in an empty one. Clauseo must not change the lawyer's own config. With
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` the login is still read from the lawyer's default store:
//! `~/.claude/.credentials.json` on Linux and Windows (`%USERPROFILE%`, Node's `homedir()`),
//! the default Keychain entry on macOS.
//!
//! The folder is `<data>/agents/<id>/detect-scratch/<pid>-<nanos>-<n>`: made fresh for each
//! run (never shared, never reused), and removed when [`Scratch`] drops, which is also when the
//! run's future is dropped by a timeout or a cancel.
//!
//! Nothing of the lawyer's is copied in: their global config (`~/.claude.json`) and settings
//! (`~/.claude/settings.json`) can hold an API key or a key helper, and Clauseo never opens or
//! reads a credential (decision 0046). So the CLI sees an empty config folder: the account's
//! email (kept in the global config) is not reported, and a login set up in the settings is not
//! seen. "Use it"'s status under the reused environment supplies what it can.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clauseo_protocol::AgentId;

/// The folder under `<data>/agents/<id>` the runs' folders are made in.
const PARENT: &str = "detect-scratch";

/// One more for each folder this process makes, so two runs in one nanosecond differ too.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One detection run's config folder, removed on drop.
#[derive(Debug)]
pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Makes a new, empty folder for one run under `data_dir`, through no link
    /// (`crate::write::create_private_dirs_under`, mode 0700), named for `now` (the config's
    /// clock), this process and a counter. Err when the name is taken: a folder is never shared.
    pub(crate) fn create(data_dir: &Path, agent: &AgentId, now: SystemTime) -> io::Result<Scratch> {
        let parent = data_dir.join("agents").join(agent.as_str()).join(PARENT);
        let nanos = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let n = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{}-{nanos}-{n}", std::process::id()));
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the detection folder's name is taken",
            ));
        }
        crate::write::create_private_dirs_under(data_dir, &path)?;
        Ok(Scratch { path })
    }

    /// The folder: the run's `CLAUDE_CONFIG_DIR`.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(err) = crate::write::remove_dir_all(&self.path) {
            tracing::warn!(error = %err, "could not remove a sign-in detection folder");
        }
    }
}

#[cfg(test)]
mod tests;
