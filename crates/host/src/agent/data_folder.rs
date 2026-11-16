//! The launch refusal of a matter folder in Clauseo's own data folder, for every agent. iris's
//! #155 refuses the whole data tree at import; this launch check is the second line, for matters
//! imported before #155 and for paths that change after import (a link pointed elsewhere). It
//! refuses a matter folder that overlaps `<data>/agents`: one that is the data folder or holds
//! it, is `agents` or holds it, or sits inside it (any agent's home, a sibling's included), so no
//! agent can read a login or plant what Codex loads from a home. It refuses the same overlap with
//! `<data>/browser` (every session's guard folder, Chrome's profile in it) and `<data>/browsers`
//! (the Chrome for Testing the guard runs): the matter folder is allowed to the agent's shell, and
//! an allow on the very path the sandbox denies would reopen it (`agent::loads::sandbox`). The
//! gap, named: a matter elsewhere inside the data folder still launches; it holds no logins.

use std::path::Path;

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// The folders of `<data>` no matter folder may overlap: the agents' homes and installs, every
/// session's guard folder (`crate::browser::CONFIG_DIR`) and the Chrome for Testing downloads
/// (`crate::browser::chrome::BROWSERS_DIR`).
const GUARDED: [&str; 3] = [
    "agents",
    crate::browser::CONFIG_DIR,
    crate::browser::chrome::BROWSERS_DIR,
];

/// Refuses the matter folder `stored` when it and any of [`GUARDED`] under `data` are the
/// same folder or one holds the other, as `clauseo_matters::same_or_nested` compares them
/// (canonicalised, case-insensitively on macOS and Windows), with `matter.folder_refused` and
/// the one message. It is checked against those folders, never `data` itself: the comparison
/// is symmetric, and a matter inside the data folder elsewhere (the tests' matters) is not
/// refused. `data` is canonicalised first, since it exists and the folders may not yet, so a
/// linked or case-different spelling still matches; a `data` that does not resolve refuses,
/// failing closed. The paths go to the log only. It stats the file system: run it off the actor.
pub(crate) fn refuse_data_folder(stored: &Path, data: &Path) -> Result<(), Error> {
    let overlaps = match dunce::canonicalize(data) {
        Ok(data) => GUARDED
            .iter()
            .any(|name| clauseo_matters::same_or_nested(stored, &data.join(name))),
        Err(error) => {
            tracing::warn!(%error, "the data folder does not resolve");
            true
        }
    };
    if !overlaps {
        return Ok(());
    }
    tracing::warn!("an agent was refused: the matter folder overlaps Clauseo's data folder");
    Err(Error::refused(
        ErrorCode::MatterFolderRefused,
        "An agent can't work in this matter: its folder holds Clauseo's own data, where agents \
         keep their sign-in.",
    ))
}

#[cfg(test)]
mod tests;
