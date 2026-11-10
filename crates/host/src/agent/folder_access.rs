//! Whether Clauseo may open a matter folder at all, for `matter.folder_permission`. On macOS,
//! privacy protection (TCC) keeps an app out of Documents, Desktop, Downloads, removable and
//! network volumes and iCloud Drive until the lawyer allows it in System Settings › Privacy &
//! Security › Files and Folders. An agent the host starts inherits Clauseo's answer, as Clauseo
//! is its responsible process (nothing in `clauseo_process` disclaims that), so a folder the
//! host may not open is one the agent may not open either. Without this check such a launch
//! started the agent, and Claude's adapter answered `session/new` with -32602 "`cwd` does not
//! exist", which reached the lawyer as `agent.protocol_error`.
//!
//! Two places give the code:
//!
//! - [`preflight`], in a launch's first folder check (`actor/turn/launch_checks/folder.rs`), off
//!   the actor: one `opendir` of the matter folder ([`open_folder`], macOS only; elsewhere it
//!   opens nothing and never refuses), no entry read. A refusal of it (`PermissionDenied`:
//!   EPERM, which TCC gives, or EACCES) refuses the launch, so no agent is started. With the
//!   usage strings in the app's `Info.plist` (`crates/desktop/Info.plist`), that open is also
//!   what makes macOS ask the lawyer, the first time, instead of refusing in silence.
//! - [`session_refusal`], for a launch that failed on an agent's error answer
//!   (`actor/turn/agents/start_failure.rs`, after its log line): only the shape of Claude's
//!   `validateCwd` (an answer to `session/new` with JSON-RPC -32602, `data` holding `cwd`, and
//!   the message saying the `cwd` does not exist) becomes the code, on every OS, since the host
//!   found the folder there just before; any other answer stays the ACP crate's code
//!   (`agent.protocol_error`).
//!
//! Neither the refusal nor the log names the folder: the agent's message holds the path, so it
//! is never logged or passed on.

use std::io;
use std::path::Path;

use clauseo_protocol::{ErrorCode, HostError};

use crate::error::Error;

/// The refusal's detail, for logs; the app's words come from the code.
const DETAIL: &str = "the computer's privacy settings keep Clauseo from opening the matter folder";

/// JSON-RPC's invalid params code, which Claude's adapter answers a missing `cwd` with.
const INVALID_PARAMS: i32 = -32602;

/// The request Claude's adapter checks the `cwd` of (`session/load` too, but a load that fails
/// is followed by a `session/new`, whose failure is the launch's).
const SESSION_NEW: &str = "session/new";

/// The key of the answer's `data` that names the `cwd` (`RequestError.invalidParams({ cwd })`).
const CWD_KEY: &str = "cwd";

/// What Claude's adapter (`claude-agent-acp`, `validateCwd`) says when its `fs.stat` of the
/// `cwd` fails, before the path it appends.
const CWD_MISSING: &str = "`cwd` does not exist";

/// The refusal: `matter.folder_permission`, with no path.
pub(crate) fn refusal() -> Error {
    Error::Refused(HostError::new(ErrorCode::MatterFolderPermission, DETAIL))
}

/// Refuses a launch in `folder` when `open` (in the host, [`open_folder`]) is refused
/// permission: `matter.folder_permission`. Any other failure is let through, for the other
/// checks to report (a folder that is gone is `matter.folder_missing` already) or the spawn.
/// Logs the error's number, never the folder. Its file system call: off the actor.
pub(crate) fn preflight(
    folder: &Path,
    open: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), Error> {
    match open(folder) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            tracing::warn!(
                errno = ?error.raw_os_error(),
                "the matter folder did not open: the privacy settings keep Clauseo out of it, so \
                 no agent was started"
            );
            Err(refusal())
        }
        Err(error) => {
            tracing::info!(
                error_kind = ?error.kind(),
                "the matter folder did not open; the other checks report it"
            );
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

/// macOS: opens `folder` as a directory (`opendir`) and closes it, reading no entry. This is the
/// access TCC decides, and asks the lawyer about the first time. Off macOS nothing is opened: no
/// privacy protection keeps the host out of a folder there.
pub(crate) fn open_folder(folder: &Path) -> io::Result<()> {
    if cfg!(target_os = "macos") {
        std::fs::read_dir(folder).map(drop)
    } else {
        Ok(())
    }
}

/// The refusal for a launch that failed with `error`, when it is Claude's "`cwd` does not
/// exist" answer to `session/new` (-32602, `data` holding `cwd`): `matter.folder_permission`,
/// with no path. `None` for any other failure, which keeps its own code.
pub(crate) fn session_refusal(error: &Error) -> Option<Error> {
    let Error::Acp(clauseo_acp::Error::Rpc {
        method,
        code,
        message,
        data,
    }) = error
    else {
        return None;
    };
    let cwd_refused = method == SESSION_NEW
        && *code == INVALID_PARAMS
        && data_keys(data).any(|key| key == CWD_KEY)
        && message.contains(CWD_MISSING);
    cwd_refused.then(refusal)
}

/// The key names in an answer's `data` shape as the ACP crate writes it (`keys [a, b]`); none
/// for any other shape.
fn data_keys(data: &str) -> impl Iterator<Item = &str> {
    data.strip_prefix("keys [")
        .and_then(|keys| keys.strip_suffix(']'))
        .unwrap_or_default()
        .split(", ")
        .filter(|key| !key.is_empty())
}

#[cfg(test)]
mod tests;
