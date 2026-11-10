//! The settings' paths classified before any of them is touched ([`off_local`]): each name and
//! link on the way, by its text and the mount table, so a link above a folder to a server's
//! share is never followed (following it would send the user's NTLM hash or hang the launch).

use std::path::{Path, PathBuf};

use super::PolicyBuilder;
use super::build::refuse;
use crate::Error;
use crate::links::network::{Mounts, lexical, off_local};

/// The hidden paths split by where they are.
pub(super) struct Hidden {
    /// On the local disks: resolved as before.
    pub(super) local: Vec<PathBuf>,
    /// Off them (another matter on a share): kept as written, `..` taken off by text, never
    /// touched. The agent's network is proxy-only, and the deny still names the path.
    pub(super) off: Vec<PathBuf>,
}

/// The mount table to classify by: this machine's, or a test's. Fails if it can't be read.
pub(super) fn mounts(b: &PolicyBuilder) -> Result<Mounts, Error> {
    #[cfg(test)]
    let mounts = b.mounts.clone().unwrap_or_else(Mounts::current);
    #[cfg(not(test))]
    let mounts = {
        let _ = b;
        Mounts::current()
    };
    if mounts.unread {
        let error = std::io::Error::other("the mount list could not be read");
        return Err(Error::Probe(error));
    }
    Ok(mounts)
}

/// Refuses a matter off the local disks with [`Error::MatterOnNetwork`], and any other folder
/// or file the agent is given (state, home, temp, notes, credential link, Claude store, runtime,
/// read and allowed paths, read-only paths in the state folder, over-cap files) that is on a
/// network drive or a device the launch can't check.
/// Returns the hidden paths split by [`Hidden`]. Before this, nothing in `b` was touched.
pub(super) fn check(b: &PolicyBuilder, mounts: &Mounts) -> Result<Hidden, Error> {
    if let Some((_, off)) = off_local(&b.matter, mounts) {
        tracing::info!(?off, "the sandbox refused a matter off the local disks");
        return Err(Error::MatterOnNetwork);
    }
    let single = [
        (Some(&b.state), "state folder"),
        (b.home.as_ref(), "home folder"),
        (b.tmpdir.as_ref(), "temp folder"),
        (b.notes.as_ref(), "notes folder"),
        (b.credential_link.as_ref(), "credential link"),
        (b.claude_store.as_ref(), "Claude store folder"),
    ];
    let many = [
        (&b.runtime, "runtime path"),
        (&b.read, "read path"),
        (&b.allowed_folders, "allowed folder"),
        (&b.read_only_in_state, "read-only path in the state folder"),
        (&b.over_cap, "over-cap file"),
    ];
    let each = single
        .into_iter()
        .filter_map(|(path, what)| path.map(|path| (path, what)))
        .chain(
            many.into_iter()
                .flat_map(|(paths, what)| paths.iter().map(move |path| (path, what))),
        );
    for (path, what) in each {
        local(path, what, mounts)?;
    }
    let (off, local): (Vec<&PathBuf>, Vec<&PathBuf>) = b
        .hidden
        .iter()
        .partition(|path| off_local(path, mounts).is_some());
    if !off.is_empty() {
        tracing::info!(
            count = off.len(),
            "hidden paths off the local disks are kept as written"
        );
    }
    Ok(Hidden {
        local: local.into_iter().cloned().collect(),
        off: off.into_iter().map(|path| lexical(path)).collect(),
    })
}

/// Refuses `path` (the `what`) when it leaves the local disks.
fn local(path: &Path, what: &str, mounts: &Mounts) -> Result<(), Error> {
    match off_local(path, mounts) {
        None => Ok(()),
        Some(_) => refuse(format!(
            "the {what} is on a network drive or a drive the sandbox can't check: {}",
            path.display()
        )),
    }
}
