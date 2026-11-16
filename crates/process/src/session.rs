//! A session's marker, and the sweep that ends every process carrying it when the session ends.
//!
//! The host gives every agent launch its own [`SessionMark`]: [`SESSION_VAR`] set to 128 random
//! bits in hex, in the launch's environment. Every process the agent starts inherits it,
//! whatever session, group or parent it ends up with. [`SessionMark::sweep`] lists this user's
//! processes, reads each one's environment, and sends SIGKILL to every one holding exactly this
//! session's entry, by pid, pass after pass until a pass finds none (`session/passes.rs`).
//!
//! Only a process that carries the entry is ever signalled, and only one run by this user (real
//! and effective uid both this process's real uid), never this process itself. Just before its
//! kill a process is read again, its start, uid and entry, so a pid reused since the listing is
//! never signalled: on Linux the pid is first pinned with a pidfd, so the signal reaches the
//! process that was read or none (`os/linux/marked.rs`); on macOS the read again is the last
//! step before `kill` (`os/macos/marked.rs`).
//!
//! On Windows the sweep does nothing: the agent's Job Object holds everything it starts, and
//! the job's end takes it all.

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::Timer;

#[cfg(any(unix, test))]
pub(crate) mod passes;

/// The variable that marks every process of one agent session.
pub const SESSION_VAR: &str = "CLAUSEO_SESSION";

/// The hex digits of a marker's value: 128 bits.
const HEX_DIGITS: usize = 32;

/// One session's marker: the value of [`SESSION_VAR`] in its launch's environment, 32 lowercase
/// hex digits. Not a secret: it lets a sweep find the session's processes, nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMark {
    value: String,
}

/// What [`SessionMark::sweep`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSwept {
    /// The sweep ran: it killed `ended` processes that carried the marker, and `left` says
    /// whether its last listing still found one (its passes ran out, or one could not be
    /// killed).
    Swept {
        /// The processes it sent SIGKILL, each counted once.
        ended: usize,
        /// A process carrying the marker was still listed at the end.
        left: bool,
    },
    /// This user's processes could not be listed, so nothing was ended.
    NotListed,
    /// Nothing to sweep on this OS: on Windows the Job Object's end takes every process the
    /// session started.
    NotNeeded,
}

impl SessionMark {
    /// The marker made from 16 random bytes (the caller's randomness: `getrandom`), as 32
    /// lowercase hex digits.
    pub fn from_random(bytes: [u8; 16]) -> SessionMark {
        use std::fmt::Write as _;
        let mut value = String::with_capacity(HEX_DIGITS);
        for byte in bytes {
            // Writing to a `String` cannot fail.
            let _infallible = write!(value, "{byte:02x}");
        }
        SessionMark { value }
    }

    /// The marker `env` sets: its [`SESSION_VAR`] entry (that name exactly, as Unix compares
    /// names), when it holds 32 lowercase hex digits; `None` otherwise.
    pub fn of_env(env: &BTreeMap<OsString, OsString>) -> Option<SessionMark> {
        let value = env.get(&OsString::from(SESSION_VAR))?.to_str()?;
        let valid = value.len() == HEX_DIGITS
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        valid.then(|| SessionMark {
            value: value.to_owned(),
        })
    }

    /// The marker's value: 32 lowercase hex digits.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The variable to put in the launch's environment: [`SESSION_VAR`] and the value.
    pub fn var(&self) -> (OsString, OsString) {
        (OsString::from(SESSION_VAR), OsString::from(&self.value))
    }

    /// The whole environment entry a marked process holds: `CLAUSEO_SESSION=<value>`.
    fn entry(&self) -> Vec<u8> {
        format!("{SESSION_VAR}={}", self.value).into_bytes()
    }

    /// Sends SIGKILL to every process of this user that carries this marker, by pid, reading each
    /// again just before its kill; then lists again, at most 5 passes, until a pass finds none.
    /// Between passes it waits on `timer` (20 ms at a time, at most 200 ms) until the processes
    /// it killed have stopped running, so whatever they forked before they died is in the next
    /// listing. Blocking between those waits: each pass reads every process's environment. Run
    /// it at the session's end, after the agent's own stop, off any thread that must stay
    /// responsive. On Windows it does nothing ([`SessionSwept::NotNeeded`]).
    pub async fn sweep(&self, timer: &impl Timer) -> SessionSwept {
        crate::os::sweep_marked(&self.entry(), timer).await
    }
}

/// Whether `environ` (a process's environment block: `NAME=value` entries, each ended by a NUL)
/// holds `entry` as one whole entry, byte for byte. A longer value, another name that ends with
/// the same text, or the entry inside another entry's value never match.
#[cfg(any(unix, test))]
pub(crate) fn carries<'a>(mut entries: impl Iterator<Item = &'a [u8]>, entry: &[u8]) -> bool {
    entries.any(|held| held == entry)
}

/// The entries of an environment block: split at each NUL, empty pieces left out.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn entries(environ: &[u8]) -> impl Iterator<Item = &[u8]> {
    environ
        .split(|&byte| byte == 0)
        .filter(|piece| !piece.is_empty())
}

#[cfg(test)]
mod tests;
