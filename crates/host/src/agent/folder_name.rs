//! The matter folders an agent can't start in because of their name: on macOS and Linux, a
//! working folder whose path holds a backslash (`\`). Claude Code (2.1.293) exits at start in
//! such a folder ("Can't access working directory …: Path … does not exist"), and its adapter
//! answered `session/new` with -32603, which reached the lawyer as `agent.protocol_error`.
//! A launch's preparation (`actor/turn/launch_checks.rs`) asks [`launch_cwd`] for the folder
//! the agent would start in, which refuses it with [`refuse_backslash`], so such a launch fails
//! with `matter.folder_backslash` and no agent starts. On Windows `\` is the separator, so
//! nothing is refused there.
//!
//! The kinds refused are [`REFUSES_BACKSLASH`]'s. Neither the refusal nor the log names the
//! folder.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};

use crate::error::Error;

/// The agent kinds a backslash in the working folder's path keeps from starting. Codex is not
/// listed until it is tested in such a folder; listing it refuses it too.
pub(crate) const REFUSES_BACKSLASH: &[AgentKind] = &[AgentKind::Claude];

/// The refusal's detail, for logs; the app's words come from the code.
const DETAIL: &str = "the matter folder's path has a backslash, where the agent cannot start";

/// The working folder an agent of `kind` starts in for the matter folder `stored`, as spawn
/// takes it ([`crate::agent::session_folder`]), unless it is too deep
/// (`matter_worker::refuse_too_deep`) or has a backslash the agent can't start in
/// ([`refuse_backslash`]). Its file system call: off the actor.
pub(crate) fn launch_cwd(kind: Option<AgentKind>, stored: &Path) -> Result<PathBuf, Error> {
    let cwd = crate::agent::session_folder(stored);
    crate::matter_worker::refuse_too_deep(&cwd)?;
    refuse_backslash(kind, &cwd)?;
    Ok(cwd)
}

/// Refuses a launch of an agent of `kind` in `folder`, the folder it would start in, when that
/// kind is one of [`REFUSES_BACKSLASH`] and the path holds a backslash on this OS
/// ([`has_backslash`]): `matter.folder_backslash`, with no path. Reads no disk.
pub(crate) fn refuse_backslash(kind: Option<AgentKind>, folder: &Path) -> Result<(), Error> {
    refuse_on(kind, folder, cfg!(windows))
}

/// [`refuse_backslash`] as on Windows (`windows`) or off it, so each OS's answer is tested on
/// every OS.
fn refuse_on(kind: Option<AgentKind>, folder: &Path, windows: bool) -> Result<(), Error> {
    let refused = kind.is_some_and(|kind| REFUSES_BACKSLASH.contains(&kind));
    if refused && has_backslash(folder, windows) {
        tracing::warn!(
            ?kind,
            "the matter folder's path has a backslash, so the agent was not started"
        );
        return Err(Error::refused(ErrorCode::MatterFolderBackslash, DETAIL));
    }
    Ok(())
}

/// Whether `folder`'s text holds a backslash that is part of a name: never on Windows
/// (`windows`), where it is the separator.
pub(crate) fn has_backslash(folder: &Path, windows: bool) -> bool {
    !windows && folder.as_os_str().as_encoded_bytes().contains(&b'\\')
}

#[cfg(test)]
mod tests;
