//! The writable allowances the profile applies after the hidden paths, kept off them: the
//! Claude store's names and the browser socket folder. Seatbelt applies the last rule that
//! matches, so either one on a hidden path would write there, a hidden matter included.

use std::fmt;
use std::path::{Path, PathBuf};

use super::build::existing;
use super::other_matter::HiddenMatter;
use super::{CLAUDE_STORE_BESIDE_LOCK_SUFFIX, ClaudeStore, PolicyBuilder, under};
use crate::Error;

/// Which allowance [`Error::HiddenOverlap`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenOverlapAllowance {
    /// The Claude store's folder, or the legacy lock folder beside it (`<dir>.lock`).
    ClaudeStore,
    /// The browser socket folder.
    BrowserSocketDir,
}

impl fmt::Display for HiddenOverlapAllowance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HiddenOverlapAllowance::ClaudeStore => "Claude store folder",
            HiddenOverlapAllowance::BrowserSocketDir => "browser socket folder",
        })
    }
}

/// `b`'s Claude store canonical (by [`existing`]), refused by [`check_store`] (its folder and
/// the lock folder beside it), and its browser socket folder refused by [`check_socket_dir`].
pub(super) fn store_and_sockets(
    b: &PolicyBuilder,
    hidden: &[PathBuf],
    matters: &[HiddenMatter],
) -> Result<Option<ClaudeStore>, Error> {
    let store = b
        .claude_store
        .as_deref()
        .map(|dir| existing(dir, "Claude store folder").map(|dir| ClaudeStore { dir }))
        .transpose()?;
    if let Some(store) = &store {
        let mut beside = store.dir.clone().into_os_string();
        beside.push(CLAUDE_STORE_BESIDE_LOCK_SUFFIX);
        check_store(&[store.dir.clone(), PathBuf::from(beside)], hidden, matters)?;
    }
    if let Some(dir) = &b.browser_socket_dir {
        check_socket_dir(dir, hidden)?;
    }
    Ok(store)
}

/// Refuses a Claude store whose folder or beside lock (`store`, canonical) is on, inside or
/// above a hidden matter, or on or above any other hidden path. A store inside a plain hidden
/// path still builds: Clauseo's own login home sits in the hidden data dir, and the profile
/// allows the store's fixed names again after the hidden paths for exactly that.
fn check_store(
    store: &[PathBuf],
    hidden: &[PathBuf],
    matters: &[HiddenMatter],
) -> Result<(), Error> {
    let allowance = HiddenOverlapAllowance::ClaudeStore;
    for path in store {
        if let Some(m) = matters
            .iter()
            .find(|m| under(path, &m.path) || under(&m.path, path))
        {
            return Err(overlap(allowance, path, &m.path));
        }
        if let Some(h) = hidden.iter().find(|h| under(h, path)) {
            return Err(overlap(allowance, path, h));
        }
    }
    Ok(())
}

/// Refuses a browser socket folder (`dir`, canonical) on, inside or above any hidden path.
fn check_socket_dir(dir: &Path, hidden: &[PathBuf]) -> Result<(), Error> {
    match hidden.iter().find(|h| under(dir, h) || under(h, dir)) {
        Some(h) => Err(overlap(HiddenOverlapAllowance::BrowserSocketDir, dir, h)),
        None => Ok(()),
    }
}

fn overlap(allowance: HiddenOverlapAllowance, path: &Path, hidden: &Path) -> Error {
    Error::HiddenOverlap {
        allowance,
        path: path.to_owned(),
        hidden: hidden.to_owned(),
    }
}
