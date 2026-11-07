//! What the two runners say to each other on the `ctl` pipe, and the agent's command line.
//!
//! A frame is a little-endian `u32` length, then that many bytes of JSON, at most
//! [`LAUNCH_CAP`](super::LAUNCH_CAP). The outer sends one [`InnerLaunch`]; the inner answers one
//! [`InnerReport`]. Nothing else crosses `ctl`.

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::LAUNCH_CAP;
use crate::windows::sids::Sid;

/// What the outer runner sends the inner runner: everything it needs to start the agent, the
/// environment already made ([`super::agent_env`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerLaunch {
    /// The pool user the inner runner must be.
    pub pool_user: Sid,
    /// The desktop user, for the agent's default DACL.
    pub desktop_user: Sid,
    /// The matter's SID, restricting too when set.
    pub matter_sid: Option<Sid>,
    /// The program, absolute: `CreateProcessAsUserW`'s application name.
    pub program: String,
    /// The whole command line, the program first ([`command_line`]).
    pub command_line: String,
    /// The working folder.
    pub cwd: String,
    /// The agent's whole environment, sorted.
    pub env: Vec<(String, String)>,
    /// The state folder's places to make first, as the pool user (`env::profile_folders`).
    pub folders: Vec<String>,
    /// The folder they all lie below, which exists: the state folder.
    pub folder_base: String,
    /// The private desktop the outer runner made for the agent, `WinSta0\<name>`.
    pub desktop: String,
}

impl std::fmt::Debug for InnerLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerLaunch")
            .field("pool_user", &self.pool_user)
            .field("program", &self.program)
            .field("env", &self.env.len())
            .finish_non_exhaustive()
    }
}

/// What the inner runner answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InnerReport {
    /// The agent started; its pid.
    Started {
        /// The agent's process id.
        pid: u32,
    },
    /// The agent didn't start: the [`RunExit`](super::RunExit) code the inner exits with, and
    /// what failed (a call and its code, never a path or a value).
    Failed {
        /// The exit code.
        exit: u32,
        /// What failed.
        what: String,
    },
    /// The agent exited, with this code: sent after `Started`, so the outer can tell the agent's
    /// own end (a start that failed in it, `0xC0000142`) from the inner runner's.
    Exited {
        /// The agent's exit code.
        code: u32,
    },
}

/// Writes `value` as one frame.
pub fn write_frame<T: Serialize>(mut to: impl Write, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len())
        .ok()
        .filter(|len| u64::from(*len) <= LAUNCH_CAP)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut framed = Vec::with_capacity(bytes.len() + 4);
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(&bytes);
    to.write_all(&framed)?;
    to.flush()
}

/// Reads one frame as a `T`; refuses one over the cap, a short one, or bad JSON.
pub fn read_frame<T: DeserializeOwned>(mut from: impl Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    from.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if u64::from(len) > LAUNCH_CAP {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut bytes = vec![0u8; usize::try_from(len).map_err(io::Error::other)?];
    from.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The command line `CreateProcessAsUserW` gets: the program in quotes (a path holds no `"`),
/// then each argument quoted as the C runtime and `CommandLineToArgvW` read it back.
pub fn command_line(program: &str, args: &[String]) -> String {
    let mut line = format!("\"{program}\"");
    for arg in args {
        line.push(' ');
        quote_into(&mut line, arg);
    }
    line
}

/// Appends `arg`, quoted if it needs to be: backslashes doubled before a quote and at the end,
/// each quote escaped.
fn quote_into(line: &mut String, arg: &str) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        line.push_str(arg);
        return;
    }
    line.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                line.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                line.push('"');
                backslashes = 0;
            }
            _ => {
                line.extend(std::iter::repeat_n('\\', backslashes));
                line.push(c);
                backslashes = 0;
            }
        }
    }
    line.extend(std::iter::repeat_n('\\', backslashes * 2));
    line.push('"');
}
