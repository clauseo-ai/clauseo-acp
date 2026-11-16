//! How each sign-in run is stopped when its job is stopped gracefully ([`crate::cancel::Stop`]:
//! the app's quit, the lawyer's Cancel), and what is checked after a run that may write Codex's
//! login file is killed.
//!
//! Codex saves its login file by truncating it and writing it in place, so a kill in that
//! window leaves it empty or cut. Each run kind therefore ends the gentlest way it has
//! ([`Stopping`]):
//! - Codex's browser `login`: `GET /cancel` on its callback port, which ends the login with no
//!   write, and waits behind a save already going (the login answers one request at a time).
//!   Sent only while the port's listening socket is owned by the login's own process tree
//!   ([`owner`]); when that cannot be confirmed nothing is sent and the run is killed at the
//!   bound.
//! - Codex's `app-server`: its stdin closed (an idle one exits at once).
//! - `login status` (both vendors) and Codex's `logout`: awaited, never cancelled.
//! - Codex's device-code login and Claude's login: killed at once, as before (no graceful path).
//!
//! A run still going at the bound is killed, and logged by its kind alone. After any kill of a
//! Codex run that may write the login file (`login`, `logout`, `app-server`), the login file of
//! the home that run used (its own `CODEX_HOME`, nothing else) is removed when, and only when, a
//! stat says it is 0 bytes ([`super::reuse::remove_empty_home_login`]); its contents are never
//! read.

pub(crate) mod owner;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use clauseo_protocol::AgentId;

use super::reuse::Empty;

/// The connect, write and read timeout of the `GET /cancel`.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(1);

/// How long a 0-byte login file must have been untouched before a status read removes it: a
/// Codex writing it now (a session refreshing its token) truncates and writes within
/// microseconds, so a file this old and empty is a cut save, never one being written.
pub(crate) const EMPTY_AGE: Duration = Duration::from_secs(5);

/// How a run ends when its job is stopped gracefully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stopping {
    /// Killed at once (device-code login, Claude's login).
    Kill,
    /// Left to finish (`login status`, `logout`); killed only at the bound.
    Wait,
    /// Codex's browser login: `GET /cancel` on this port when the login owns it, then awaited.
    Cancel {
        /// The callback port.
        port: u16,
    },
}

/// What a run's stop needs: its kind, the words the log names it by, and whether it may write
/// Codex's login file in its home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OnStop {
    pub(crate) how: Stopping,
    /// Fixed words for the log, such as `codex login status`: never a path or an account.
    pub(crate) what: &'static str,
    /// Whether the run may write Codex's login file in its `CODEX_HOME`.
    pub(crate) writes: bool,
}

impl OnStop {
    /// Killed at once, writing nothing Codex keeps: the default.
    pub(crate) const KILL: OnStop = OnStop {
        how: Stopping::Kill,
        what: "run",
        writes: false,
    };

    /// A status run: awaited, writes nothing.
    pub(crate) fn status(what: &'static str) -> OnStop {
        OnStop {
            how: Stopping::Wait,
            what,
            writes: false,
        }
    }
}

/// The `CODEX_HOME` a run's environment sets, when it sets one that is not empty: the one home
/// whose login file that run could write. Detection's runs set none (they look at the lawyer's
/// own folder), so nothing is ever removed there.
pub(crate) fn codex_home_of(env: &BTreeMap<OsString, OsString>) -> Option<PathBuf> {
    env.get(&OsString::from("CODEX_HOME"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// After a kill of a run that may write: the login file of the home that run used is removed
/// when it is 0 bytes and not the lawyer's (`lawyer_home`'s), with a fixed-word log naming the
/// agent.
pub(crate) fn after_kill(
    on_stop: OnStop,
    env: &BTreeMap<OsString, OsString>,
    lawyer_home: Option<&Path>,
    agent: &AgentId,
) {
    if !on_stop.writes {
        return;
    }
    let Some(home) = codex_home_of(env) else {
        return;
    };
    super::reuse::remove_empty_home_login(&home, lawyer_home, Empty::AfterKill, agent);
}

/// Before a status read in a Clauseo home (`home`, a run's `CODEX_HOME`): its login file is
/// removed when it is 0 bytes and has been so for [`EMPTY_AGE`] by `now`, a cut save from a kill
/// this process never saw (a crash, a power cut, the OS's own end of a session).
pub(crate) fn before_status(
    home: &Path,
    lawyer_home: Option<&Path>,
    now: SystemTime,
    agent: &AgentId,
) {
    let cutoff = now.checked_sub(EMPTY_AGE).unwrap_or(SystemTime::UNIX_EPOCH);
    let when = Empty::AtStatus { cutoff };
    super::reuse::remove_empty_home_login(home, lawyer_home, when, agent);
}

/// Sends `GET /cancel` to `127.0.0.1:port` by plain HTTP over a local connection: no proxy,
/// [`CANCEL_TIMEOUT`] to connect, write and read, every error ignored. Blocks for at most about
/// three times that; the read waits for the login's answer, which comes after a save already
/// going.
pub(crate) fn send_cancel(port: u16) {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CANCEL_TIMEOUT) else {
        return;
    };
    if stream.set_write_timeout(Some(CANCEL_TIMEOUT)).is_err()
        || stream.set_read_timeout(Some(CANCEL_TIMEOUT)).is_err()
    {
        return;
    }
    let request =
        format!("GET /cancel HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return;
    }
    // The answer is not read for anything: the read only waits for it, or for the timeout.
    let mut answer = [0_u8; 512];
    drop(stream.read(&mut answer));
}

#[cfg(test)]
mod tests;
