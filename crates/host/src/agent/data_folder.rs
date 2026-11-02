//! The launch refusal of a matter folder in Clauseo's own data folder, for every agent. iris's
//! #155 refuses the whole data tree at import; this launch check is the second line, for matters
//! imported before #155 and for paths that change after import (a link pointed elsewhere). It
//! refuses a matter folder that overlaps `<data>/agents`: one that is the data folder or holds
//! it, is `agents` or holds it, or sits inside it (any agent's home, a sibling's included), so no
//! agent can read a login or plant what Codex loads from a home. The gap, named: a matter
//! elsewhere inside the data folder still launches; it holds no logins.

use std::path::Path;

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// Refuses the matter folder `stored` when it and `<data>/agents` are the same folder or one
/// holds the other, as `clauseo_matters::same_or_nested` compares them (canonicalised,
/// case-insensitively on macOS and Windows), with `matter.folder_refused`. It is checked
/// against `agents`, never `data` itself: the comparison is symmetric, and a matter inside the
/// data folder elsewhere (the tests' matters) is not refused. `data` is canonicalised first,
/// since it exists and `agents` may not yet, so a linked or case-different spelling still
/// matches; a `data` that does not resolve refuses, failing closed. The paths go to the log
/// only. It stats the file system: run it off the actor.
pub(crate) fn refuse_data_folder(stored: &Path, data: &Path) -> Result<(), Error> {
    let overlaps = match dunce::canonicalize(data) {
        Ok(data) => clauseo_matters::same_or_nested(stored, &data.join("agents")),
        Err(error) => {
            tracing::warn!(data = %data.display(), %error, "the data folder does not resolve");
            true
        }
    };
    if !overlaps {
        return Ok(());
    }
    tracing::warn!(
        folder = %stored.display(),
        data = %data.display(),
        "an agent was refused: the matter folder overlaps Clauseo's data folder"
    );
    Err(Error::refused(
        ErrorCode::MatterFolderRefused,
        "An agent can't work in this matter: its folder holds Clauseo's own data, where agents \
         keep their sign-in.",
    ))
}

#[cfg(test)]
mod tests;
