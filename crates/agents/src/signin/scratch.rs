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
//! What detection reports depends on two of the lawyer's files the CLI would read from its
//! default config folder, so copies of them are put in the folder first ([`Scratch::seed`]),
//! each opened read-only and never written back:
//! - the global config, `~/.claude.json` (or the legacy `~/.claude/.config.json`, which wins when
//!   present, as in the CLI): it holds the account (`oauthAccount`) whose email detection shows;
//!   on Windows it is `%USERPROFILE%\.claude.json`;
//! - the user settings, `~/.claude/settings.json`: a login set up there (`apiKeyHelper`, say) is
//!   detection's `found: ClaudeSettings`.
//!
//! A copy that cannot be made is skipped: detection then runs without it (no email shown, say),
//! never on the lawyer's own folder.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clauseo_protocol::AgentId;

/// The folder under `<data>/agents/<id>` the runs' folders are made in.
const PARENT: &str = "detect-scratch";

/// The largest file copied in: the global config grows with the lawyer's project history.
const COPY_LIMIT: u64 = 64 * 1024 * 1024;

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

    /// Copies the lawyer's global config and user settings from `lawyer_home` into the folder
    /// under the names the CLI reads there (see the module docs). Missing files are skipped,
    /// as is any copy that fails.
    pub(crate) fn seed(&self, lawyer_home: &Path) {
        let claude = lawyer_home.join(".claude");
        for (from, to) in [
            (claude.join(".config.json"), ".config.json"),
            (lawyer_home.join(".claude.json"), ".claude.json"),
            (claude.join("settings.json"), "settings.json"),
        ] {
            if let Ok(Some(bytes)) = read_lawyer_file(&from) {
                drop(crate::write::write_atomic(&self.path.join(to), &bytes));
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(err) = crate::write::remove_dir_all(&self.path) {
            tracing::warn!(error = %err, "could not remove a sign-in detection folder");
        }
    }
}

/// The bytes of the lawyer's file at `path`, opened read-only; `Ok(None)` when it is missing or
/// past [`COPY_LIMIT`]. A link is followed, as the CLI follows it: only reads reach it.
fn read_lawyer_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if !file.metadata()?.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(COPY_LIMIT.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > COPY_LIMIT {
        return Ok(None);
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
