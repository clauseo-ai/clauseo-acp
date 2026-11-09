//! The agents' shell commands (`<data>/tools`, [`ProfilePaths::tools`]): the app copies the
//! bundled agent-browser sidecar there as `agent-browser`, so the command has the name its skill
//! and the instructions use, in a folder that holds nothing else for the agents' `PATH`. A copy,
//! never a link: a link would reach into the app's own folder, and the folder holds a program
//! only the app writes.

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::ProfileError;
use crate::paths::ProfilePaths;

/// Copies `source` (the bundled sidecar) to [`ProfilePaths::agent_browser_command`], only when
/// the copy is missing or its SHA-256 differs from `source`'s, through a temp file beside it and
/// a rename, mode 0755 on Unix. Returns the copy's path and whether it wrote. Reads up to two
/// programs (about 18 MB each) and may write one: at app start, off the host thread. An error
/// leaves the browser out; the caller logs it and goes on.
pub fn install_agent_browser(
    source: &Path,
    paths: &ProfilePaths,
) -> Result<(PathBuf, bool), ProfileError> {
    let target = paths.agent_browser_command();
    crate::write::sweep_old(paths.tools());
    let bytes = std::fs::read(source).map_err(|err| ProfileError::io("read", source, err))?;
    let want = Sha256::digest(&bytes);
    let held = std::fs::read(&target)
        .ok()
        .map(|held| Sha256::digest(&held));
    if held.as_ref() == Some(&want) {
        return Ok((target, false));
    }
    crate::write::replace_program(&target, &bytes)?;
    Ok((target, true))
}
