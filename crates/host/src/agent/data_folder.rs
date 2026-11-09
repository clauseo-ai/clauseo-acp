//! The launch refusal of a matter folder in Clauseo's own data folder, for every agent. iris's
//! #155 refuses the whole data tree at import; this launch check is the second line, for matters
//! imported before #155 and for paths that change after import (a link pointed elsewhere). It
//! refuses a matter folder that overlaps `<data>/agents` or `<data>/agent-state`: one that is
//! the data folder or holds it, is either folder or holds it, or sits inside it (any agent's
//! home, a sibling's included, any matter's state folder or notes), so no agent can read a
//! login, reach a matter's link to the lawyer's live Codex login (R17 PR-C) or plant what an
//! agent loads from its home or state folder. The gap, named: a matter elsewhere inside the
//! data folder still launches; it holds no logins.

use std::path::Path;

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// Refuses the matter folder `stored` when it and `<data>/agents`, or it and
/// `<data>/agent-state`, are the same folder or one holds the other, as
/// `clauseo_matters::same_or_nested` compares them (canonicalised, case-insensitively on macOS
/// and Windows), with `matter.folder_refused`. It is checked against those two, never `data`
/// itself: the comparison is symmetric, and a matter inside the data folder elsewhere (the
/// tests' matters) is not refused. `data` is canonicalised first, since it exists and the two
/// may not yet, so a linked or case-different spelling still matches; a `data` that does not
/// resolve refuses, failing closed. The paths go to the log only. It stats the file system:
/// run it off the actor.
pub(crate) fn refuse_data_folder(stored: &Path, data: &Path) -> Result<(), Error> {
    let overlaps = match dunce::canonicalize(data) {
        Ok(data) => ["agents", super::state_folder::STATE_ROOT]
            .iter()
            .any(|tree| clauseo_matters::same_or_nested(stored, &data.join(tree))),
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
