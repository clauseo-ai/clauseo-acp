//! The guard's listen options, at the front of its command line: `--listen <socket>` on Unix
//! (`transport/unix.rs`), `--listen-pipe <name> --client-sid <SID>` on Windows
//! (`transport/windows.rs`). Each OS takes its own and refuses the other's; no refusal names the
//! path, the pipe or the SID.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::env;
#[cfg(windows)]
use super::{EXIT_PIPE_TAKEN, failed, usage};
use crate::transport::checks;
#[cfg(windows)]
use crate::{Failure, transport::windows::MakeError};

/// Where listen mode serves.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Listen {
    /// Unix: `--listen <socket>`.
    Socket(PathBuf),
    /// Windows: `--listen-pipe <name> --client-sid <SID>`.
    Pipe { name: String, client_sid: String },
}

/// The refusals of `--listen`, `--listen-pipe` and `--client-sid`. None names the value.
pub(super) const LISTEN_UNIX_ONLY: &str = "--listen is for macOS and Linux only";
const LISTEN_ABSOLUTE: &str = "--listen must be an absolute path";
pub(super) const LISTEN_PIPE_WINDOWS_ONLY: &str =
    "--listen-pipe and --client-sid are for Windows only";
const LISTEN_PIPE_EXPECTED: &str = "expected --listen-pipe <name> --client-sid <sid> first";
const PIPE_NAME_BAD: &str = "--listen-pipe must be 1 to 200 letters, digits and dashes";
const CLIENT_SID_BAD: &str = "--client-sid must be a SID string, S-1-...";

/// The listen option at the front of `args`, if any, and the arguments after it. Each OS's own
/// option only; the other's is refused.
pub(super) fn listen_option(
    args: &[OsString],
    os: env::Os,
) -> Result<(Option<Listen>, &[OsString]), &'static str> {
    let pipe_flag = |flag: &OsString| flag == "--listen-pipe" || flag == "--client-sid";
    match (os, args) {
        (env::Os::Unix, [flag, socket, rest @ ..]) if flag == "--listen" => {
            let socket = Path::new(socket);
            if !socket.is_absolute() {
                return Err(LISTEN_ABSOLUTE);
            }
            Ok((Some(Listen::Socket(socket.to_owned())), rest))
        }
        (env::Os::Unix, [flag, ..]) if pipe_flag(flag) => Err(LISTEN_PIPE_WINDOWS_ONLY),
        (env::Os::Windows, [flag, name, sid_flag, sid, rest @ ..])
            if flag == "--listen-pipe" && sid_flag == "--client-sid" =>
        {
            let name = name
                .to_str()
                .filter(|name| checks::pipe_name_ok(name))
                .ok_or(PIPE_NAME_BAD)?;
            let sid = sid
                .to_str()
                .filter(|sid| checks::sid_text_ok(sid))
                .ok_or(CLIENT_SID_BAD)?;
            let pipe = Listen::Pipe {
                name: name.to_owned(),
                client_sid: sid.to_owned(),
            };
            Ok((Some(pipe), rest))
        }
        (env::Os::Windows, [flag, ..]) if pipe_flag(flag) => Err(LISTEN_PIPE_EXPECTED),
        (env::Os::Windows, [flag, ..]) if flag == "--listen" => Err(LISTEN_UNIX_ONLY),
        _ => Ok((None, args)),
    }
}

/// The failure for a pipe the guard could not make: [`EXIT_PIPE_TAKEN`] for a name that exists.
#[cfg(windows)]
pub(super) fn pipe_failure(error: MakeError) -> Failure {
    match error {
        MakeError::Taken => Failure {
            code: EXIT_PIPE_TAKEN,
            message: "clauseo-mcp browser-guard: the --listen-pipe name exists already; the guard \
                      makes the pipe fresh and never joins one"
                .to_owned(),
        },
        MakeError::BadSid => usage(CLIENT_SID_BAD),
        MakeError::Failed(reason) => failed(&reason),
    }
}
