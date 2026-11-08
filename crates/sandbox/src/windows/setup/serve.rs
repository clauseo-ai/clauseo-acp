//! `clauseo-sandbox-setup.exe --serve-set-up`, run by the SYSTEM task `\Clauseo\SandboxSetUp`:
//! the set-up a standard user asks for, with no prompt. The app starts the task
//! (`windows::setup_task`), then asks over the pipe [`PIPE_NAME`]; this process learns who asked
//! from the pipe and the Windows session, never from an argument, and sets that user up as
//! `--set-up` would, writing every file in their data folder as them.
//!
//! [`run`] holds the order and the refusals, with no Windows call; every effect goes through
//! [`Pipe`] and [`Host`], which `windows_pipe.rs` and `windows_host.rs` implement and the tests
//! fake. It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path, as the stop
//!    task does ([`ServeExit::NotAllowed`]);
//! 2. reads the record: with no admin copy recorded there is no one to serve
//!    ([`ServeExit::NoRecord`]);
//! 3. makes the pipe, its first instance or nothing (`FILE_FLAG_FIRST_PIPE_INSTANCE`), local
//!    clients only, owned by LOCAL SYSTEM, with a protected DACL built now from the record
//!    ([`pipe_sddl`]): per user the owner, per machine interactive users, the pool's group refused
//!    first once it exists. A name someone else made first is logged and served to no one
//!    ([`ServeExit::Squatted`]);
//! 4. serves each client in turn ([`handle`]), waiting [`FIRST_WAIT`] for the first and
//!    [`IDLE_WAIT`] for each after, then exits [`ServeExit::Done`]. While one is served the next
//!    can connect and wait; the task queues a start made as this process ends, and the app
//!    starts it again when the pipe stays missing, so no request is dropped.
//!
//! Each client is served in this order, each failure a reply and one log line naming no user,
//! SID or path:
//!
//! 1. its request, one message of at most [`REQUEST_CAP`] bytes read within [`READ_WAIT`]: an
//!    op-tagged, versioned JSON object (`protocol.rs`, [`Op`]), `set_up` or `add`;
//!    `ensure_slots` (reserved for several owners) and anything unknown are answered
//!    `unsupported` once the client is identified. The log names the op by its tag alone, never
//!    with text the client sent;
//! 2. who it is ([`ClientFacts`]): its token's user, groups, restriction and integrity, read at
//!    identification level (`ImpersonateNamedPipeClient`, `OpenThreadToken`), and its session
//!    (`GetNamedPipeClientSessionId`), then the session's user (`WTSQueryUserToken`);
//! 3. the gate ([`identify`]), which refuses a pool user or a member of the pool's group whatever
//!    the task's descriptor or the pipe's DACL say, session 0, a SID that isn't a person's account
//!    (`Sid::is_user_account`: local, AD, Entra ID), a restricted or Low token, a client that
//!    isn't its session's user, per user anyone but `AdminCopyOwner`, and a network or
//!    non-interactive logon;
//! 4. the data folder: the identified user's `ProfileImagePath` (the only profile read) and the
//!    fixed [`DATA_SUFFIX`], inside the profile, resolved as the user (`exe::owners_data_dir`,
//!    run with the session's token, lowered when elevated), so a link in the profile can't send
//!    a write of SYSTEM's anywhere;
//! 5. the set-up lock, waited for (the set-up only tries it), then the gate again on a fresh read
//!    of the record (a removal may have run meanwhile);
//! 6. a user whose set-up is current gets `Done` with nothing run (a re-run resets every pool
//!    password); an extend always runs;
//! 7. `exe::run_set_up`'s work for that user, whose outcome ([`SetupResult`]) is the reply, and
//!    is also written to `setup-result.json` as the user.
//!
//! The reply is one line of JSON ([`Reply`]), tagged `ran`, `refused` or `unsupported`. Pure
//! apart from its seams: tested on every OS.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::outcome::SetupExit;
use super::result_file::SetupResult;
use crate::windows::refresh::same_path;
use crate::windows::sids::Sid;

pub use identify::{
    ClientFacts, Gate, Refusal, ServerFacts, check_owner, check_server, identify, pipe_sddl,
};
pub use protocol::{
    Op, PROTOCOL, REPLY_CAP, REQUEST_CAP, Reply, SetUpError, retry_squatted, via_task,
};

mod identify;
mod protocol;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(windows)]
pub(crate) mod windows_host;
#[cfg(windows)]
pub(crate) mod windows_pipe;
#[cfg(test)]
#[cfg(windows)]
mod windows_tests;

/// The pipe's name.
pub const PIPE_NAME: &str = r"\\.\pipe\ClauseoSandboxSetUp";
/// How long the server waits for its first client: a start with no client ends here.
pub const FIRST_WAIT: Duration = Duration::from_secs(30);
/// How long it waits for each client after one was served.
pub const IDLE_WAIT: Duration = Duration::from_secs(5);
/// How long a connected client has to send its request: short, as a client holding its
/// instance unsent keeps the next one waiting.
pub const READ_WAIT: Duration = Duration::from_secs(2);
/// How long the app waits for its reply once the request is sent: the set-up task's execution
/// limit (`refresh::SET_UP_EXECUTION_LIMIT`, 30 minutes, past which the Task Scheduler ends the
/// server) and a minute more, so a set-up that runs to the limit is never cut short by the app.
pub const REPLY_WAIT: Duration = Duration::from_mins(31);
/// How long the app tries the pipe for, in milliseconds (`windows::setup_task`).
pub const CONNECT_WAIT: u64 = 30_000;
/// After how long without a pipe the app starts the task once more, in milliseconds.
pub const RERUN_AFTER: u64 = 5_000;
/// The data folder below the user's profile: the app's `$LOCALDATA\ai.clauseo.desktop`.
pub const DATA_SUFFIX: [&str; 3] = ["AppData", "Local", "ai.clauseo.desktop"];

/// The server's exit codes. They start at 63, after the codes the other `\Clauseo` tasks' runs
/// of the one exe take (`StopExit` to 50, the WSL deny's 51 and 52, the sweep's 53 to 59, the
/// WSL upgrade's 60 to 62), so no number has two meanings; codes are appended from here on, never
/// renumbered. How each client fared is in its reply and the log, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeExit {
    /// 0: every client that came was answered, and none came within the last wait.
    Done,
    /// 63: not LOCAL SYSTEM, or not run from the admin copy's path; no pipe was made.
    NotAllowed,
    /// 64: someone else made the pipe's name first, or took it while no instance was left; no
    /// one was served from then on.
    Squatted,
    /// 65: the record can't be read, or holds no admin copy; no pipe was made.
    NoRecord,
    /// 66: the pipe couldn't be made, or waiting for a client failed.
    PipeFailed,
}

impl ServeExit {
    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            ServeExit::Done => 0,
            ServeExit::NotAllowed => 63,
            ServeExit::Squatted => 64,
            ServeExit::NoRecord => 65,
            ServeExit::PipeFailed => 66,
        }
    }
}

/// Why the pipe couldn't be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmade {
    /// Its name is taken: another process made it first.
    Taken,
    /// Anything else, as a line for the log.
    Failed(String),
}

/// The pipe's server end, as [`run`] uses it.
pub trait Pipe {
    /// A connected client.
    type Client;
    /// Waits up to `wait` for the next client; `None` when none came. [`Unmade::Taken`] when the
    /// instance it had to make again was someone else's name by then.
    fn next(&mut self, wait: Duration) -> Result<Option<Self::Client>, Unmade>;
    /// The client's one request message, read within [`READ_WAIT`], at most [`REQUEST_CAP`]
    /// bytes.
    fn request(&mut self, client: &mut Self::Client) -> Result<Vec<u8>, String>;
    /// Who the client is: its token at identification level, and its session.
    fn facts(&mut self, client: &mut Self::Client) -> Result<ClientFacts, String>;
    /// Sends `line` and lets the client go.
    fn reply(&mut self, client: Self::Client, line: &[u8]);
}

/// Everything else the server reads and does.
pub trait Host {
    /// The user a set-up runs for: their session's token.
    type Owner;
    /// Whether this process's token user is LOCAL SYSTEM.
    fn is_system(&mut self) -> Result<bool, String>;
    /// This process's exe, its final path.
    fn current_exe(&mut self) -> Result<PathBuf, String>;
    /// The admin copy's path.
    fn admin_copy(&mut self) -> Result<PathBuf, String>;
    /// The record's admin-copy owner, pool and group, read now ([`Gate`]).
    fn gate(&mut self) -> Result<Gate, String>;
    /// The user signed in to `session` (`WTSQueryUserToken`).
    fn session_user(&mut self, session: u32) -> Result<Sid, String>;
    /// `user`'s own profile folder (`ProfileImagePath`); `None` for none or a temporary one.
    fn profile(&mut self, user: &Sid) -> Result<Option<PathBuf>, String>;
    /// `user` with `session`'s token, lowered when elevated (`Owner::of_session`).
    fn owner(&mut self, session: u32, user: &Sid) -> Result<Self::Owner, String>;
    /// `path` resolved as `owner`, a folder `owner` owns (`exe::owners_data_dir`).
    fn data_dir(
        &mut self,
        owner: &Self::Owner,
        path: &Path,
    ) -> Result<PathBuf, (SetupExit, String)>;
    /// Waits a bounded time for the set-up lock: `Ok(false)` when another still holds it.
    fn lock(&mut self) -> Result<bool, String>;
    /// Gives the set-up lock back.
    fn unlock(&mut self);
    /// Whether `owner`'s set-up in `data_dir` is current (`setup_state`, read as `owner`).
    fn current(&mut self, owner: &Self::Owner, data_dir: &Path) -> bool;
    /// The set-up (or an extend by `add`) for `owner` in `data_dir`, under the lock already held:
    /// its outcome, also written to `setup-result.json` as `owner`.
    fn set_up(&mut self, owner: Self::Owner, data_dir: &Path, add: u8) -> SetupResult;
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// `--serve-set-up` as the module says, the pipe made by `open` with its descriptor.
pub fn run<P: Pipe>(
    host: &mut impl Host,
    open: impl FnOnce(&str) -> Result<P, Unmade>,
) -> ServeExit {
    match allowed(host) {
        Ok(true) => {}
        Ok(false) => {
            host.log(
                true,
                "--serve-set-up runs only as SYSTEM from the admin copy",
            );
            return ServeExit::NotAllowed;
        }
        Err(error) => {
            host.log(true, &format!("--serve-set-up: this process: {error}"));
            return ServeExit::NotAllowed;
        }
    }
    let sddl = match host.gate().map(|gate| pipe_sddl(&gate)) {
        Ok(Some(sddl)) => sddl,
        Ok(None) => {
            host.log(true, "--serve-set-up: no admin copy is recorded");
            return ServeExit::NoRecord;
        }
        Err(error) => {
            host.log(true, &format!("--serve-set-up: the record: {error}"));
            return ServeExit::NoRecord;
        }
    };
    let mut pipe = match open(&sddl) {
        Ok(pipe) => pipe,
        Err(Unmade::Taken) => {
            host.log(
                true,
                "--serve-set-up: another process holds the pipe's name",
            );
            return ServeExit::Squatted;
        }
        Err(Unmade::Failed(error)) => {
            host.log(true, &format!("--serve-set-up: the pipe: {error}"));
            return ServeExit::PipeFailed;
        }
    };
    let mut wait = FIRST_WAIT;
    loop {
        match pipe.next(wait) {
            Ok(Some(mut client)) => {
                let reply = handle(&mut pipe, host, &mut client);
                pipe.reply(client, &reply.to_line());
            }
            Ok(None) => return ServeExit::Done,
            Err(Unmade::Taken) => {
                host.log(true, "--serve-set-up: another process took the pipe's name");
                return ServeExit::Squatted;
            }
            Err(Unmade::Failed(error)) => {
                host.log(
                    true,
                    &format!("--serve-set-up: waiting for a client: {error}"),
                );
                return ServeExit::PipeFailed;
            }
        }
        wait = IDLE_WAIT;
    }
}

/// What one wait for a client on an armed instance came to (`windows_pipe.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// A client is connected.
    Connected,
    /// None came in the time given.
    TimedOut,
    /// One came and closed before the server reached it (`ERROR_NO_DATA` and the like).
    Gone,
}

/// Waits up to `wait` in all, by the monotonic clock `now`, for a client that stays:
/// `attempt(left)` arms the instance and waits up to `left`; a client that came and went
/// (`Gone`) is let go (`rearm`, which disconnects the instance) and the wait goes on with what
/// is left, so one client closing early never ends the server for everyone. `false` when none
/// stayed in time.
pub(crate) fn connect_within<E>(
    wait: Duration,
    mut now: impl FnMut() -> Duration,
    mut attempt: impl FnMut(Duration) -> Result<Attempt, E>,
    mut rearm: impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    let deadline = now() + wait;
    loop {
        match attempt(deadline.saturating_sub(now()))? {
            Attempt::Connected => return Ok(true),
            Attempt::TimedOut => return Ok(false),
            Attempt::Gone => {
                rearm()?;
                if now() >= deadline {
                    return Ok(false);
                }
            }
        }
    }
}

/// Whether this process may serve: LOCAL SYSTEM, running from the admin copy's path.
fn allowed(host: &mut impl Host) -> Result<bool, String> {
    Ok(host.is_system()? && same_path(&host.current_exe()?, &host.admin_copy()?))
}

/// The reply to one client, as the module says, logged. The log line names the op by its tag
/// alone ([`Op::tag`]) and holds no text the client sent: a request that can't be read is
/// logged as that, its details only in the reply.
fn handle<P: Pipe, H: Host>(pipe: &mut P, host: &mut H, client: &mut P::Client) -> Reply {
    let op = match pipe.request(client).and_then(|bytes| Op::parse(&bytes)) {
        Ok(op) => op,
        Err(why) => {
            host.log(
                true,
                "set-up request refused: a request this server can't read",
            );
            return Reply::Refused(format!("the request: {why}"));
        }
    };
    let tag = op.tag();
    let reply = answer(pipe, host, client, op);
    let line = match &reply {
        Reply::Ran(result) if result.exit == 0 => format!("set-up request ({tag}): done"),
        Reply::Ran(result) => format!(
            "set-up request ({tag}): failed (exit {}) at {}",
            result.exit,
            result.step.as_deref().unwrap_or("an unnamed step")
        ),
        // The server's own words: a refusal names no SID or path, and none echoes the client.
        Reply::Refused(why) => format!("set-up request ({tag}) refused: {why}"),
        Reply::Unsupported(_what) => format!("set-up request ({tag}) unsupported"),
    };
    host.log(
        !matches!(&reply, Reply::Ran(result) if result.exit == 0),
        &line,
    );
    reply
}

/// [`handle`]'s reply to the request `op`, unlogged.
fn answer<P: Pipe, H: Host>(pipe: &mut P, host: &mut H, client: &mut P::Client, op: Op) -> Reply {
    let facts = match pipe.facts(client) {
        Ok(facts) => facts,
        Err(why) => return Reply::Refused(format!("who asked can't be read: {why}")),
    };
    let session_user = if facts.session == 0 {
        None
    } else {
        host.session_user(facts.session).ok()
    };
    let gated = host
        .gate()
        .map_err(|why| format!("the record: {why}"))
        .and_then(|gate| {
            identify(&facts, session_user.as_ref(), &gate).map_err(|refusal| refusal.to_string())
        });
    let user = match gated {
        Ok(user) => user,
        Err(why) => return Reply::Refused(why),
    };
    // Who asked is settled once, above, before any op; only then is the op looked at.
    let add = match op {
        Op::SetUp => None,
        Op::Add(n) => Some(n),
        // TODO(multi-owner): serve `ensure_slots` once several owners share one machine.
        Op::EnsureSlots(_) => return Reply::Unsupported("the op \"ensure_slots\"".to_owned()),
        Op::Unsupported(what) => return Reply::Unsupported(what),
    };
    let failed =
        |exit: SetupExit, step: &str, why: String| Reply::Ran(SetupResult::failed(exit, step, why));
    let path = match host.profile(&user) {
        Ok(Some(profile)) => match data_dir_in(&profile) {
            Some(path) => path,
            None => {
                return failed(
                    SetupExit::Usage,
                    "data_dir",
                    "outside the profile".to_owned(),
                );
            }
        },
        Ok(None) => return failed(SetupExit::Usage, "data_dir", "no profile".to_owned()),
        Err(why) => return failed(SetupExit::Failed, "data_dir", why),
    };
    let owner = match host.owner(facts.session, &user) {
        Ok(owner) => owner,
        Err(why) => return failed(SetupExit::Failed, "owner", why),
    };
    let data_dir = match host.data_dir(&owner, &path) {
        Ok(data_dir) => data_dir,
        Err((exit, why)) => return failed(exit, "data_dir", why),
    };
    match host.lock() {
        Ok(true) => {}
        Ok(false) => {
            let why = "another set-up or removal held the lock".to_owned();
            return failed(SetupExit::LockHeld, "lock", why);
        }
        Err(why) => return failed(SetupExit::Failed, "lock", why),
    }
    let reply = locked(host, &facts, session_user.as_ref(), owner, &data_dir, add);
    host.unlock();
    reply
}

/// Steps 5 to 7, the lock held.
fn locked<H: Host>(
    host: &mut H,
    facts: &ClientFacts,
    session_user: Option<&Sid>,
    owner: H::Owner,
    data_dir: &Path,
    add: Option<u8>,
) -> Reply {
    let again = host
        .gate()
        .map_err(|why| format!("the record: {why}"))
        .and_then(|gate| identify(facts, session_user, &gate).map_err(|r| r.to_string()));
    if let Err(why) = again {
        return Reply::Refused(why);
    }
    if add.is_none() && host.current(&owner, data_dir) {
        return Reply::Ran(SetupResult::done());
    }
    Reply::Ran(host.set_up(owner, data_dir, add.unwrap_or(0)))
}

/// Whether the set-up task would set up the app's `data_dir` for a user whose own profile folder
/// is `profile` (`ProfileImagePath`, `None` for none): the folder it derives ([`data_dir_in`])
/// is that one. When not (a moved `AppData\Local`, a portable data folder, no profile), a
/// reply would say done while the app's folder stayed unset; `Err` says why, and the app asks
/// with the prompt instead, whose `--set-up` takes the folder as it is.
pub fn task_serves(profile: Option<&Path>, data_dir: &Path) -> Result<(), &'static str> {
    let profile = profile.ok_or("this user has no profile folder of their own")?;
    match data_dir_in(profile) {
        Some(derived) if same_path(&derived, data_dir) => Ok(()),
        Some(_) => {
            Err("the app's data folder isn't AppData\\Local\\ai.clauseo.desktop in the profile")
        }
        None => Err("the profile's data folder can't be named inside it"),
    }
}

/// The data folder in `profile`: [`DATA_SUFFIX`] below it, checked by its text to lie inside it
/// (`refresh::inside_profile`); what a link in the profile does is `owners_data_dir`'s check.
pub fn data_dir_in(profile: &Path) -> Option<PathBuf> {
    let dir = DATA_SUFFIX
        .iter()
        .fold(profile.to_path_buf(), |dir, part| dir.join(part));
    crate::windows::refresh::inside_profile(&dir, profile).then_some(dir)
}
