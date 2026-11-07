//! The detached end sweeper (Windows): `clauseo-sandbox-run --sweep`, started by the outer as
//! the pool user at a session's end (`end.rs`), outside the launch's job, once the job is empty.
//! It takes nothing but its flag. Its first act, as the inner's, is to shut its own primary token
//! to all but `SYSTEM` (`token::RUNNER_TOKEN_FIRST_SDDL`): it is Medium, unrestricted, with the
//! desktop user's logon SID enabled, and the handle opened for the shut keeps its rights. It then
//! shuts its default DACL and every thread it has to `SYSTEM` alone (`token::SWEEPER_*`), as the
//! inner shuts its own to the agent, so a thread the system or COM starts later is shut too. It
//! kills every process of its own user but itself (no job is kept: the session is ending) that
//! was created before it was, or that a process it killed started: a next session's processes,
//! started later by the next outer and inner, are left alone (`sweep.rs`, which also says why a
//! next inner killing this sweeper loses nothing). It makes two passes, both before the removal:
//! the second kills what got past the first, a child of a process the first killed among them.
//! Only when both went with no failure (nothing of its user that it sweeps is left) does it
//! give its user the token rights COM needs (`token::SWEEPER_COM_TOKEN_RIGHTS`). It then removes
//! what that user could leave to start at its next logon (`leftovers.rs`), the scheduled tasks
//! and BITS jobs (over COM) only when the token was widened, all on its own [`SWEEP_LIMIT`]. The
//! order is `sweeper_flow.rs`'s. Its stderr gets each kill (pid, image name, sweep point) and each
//! kind's count, which the outer relays while it lives. It exits 0 when
//! everything went, [`RunExit::Escapee`] when a process of its user's couldn't be opened or
//! ended (`sweep::end_sweeper_exit`), [`RunExit::Sweep`] when anything else failed or the time
//! ran out.
//!
//! It writes no result file: the run folder admits only the desktop user, `SYSTEM` and
//! `Administrators`, and a file the pool user could write there would be a path its other
//! processes could write too. The next launch's first-act cleanup logs what it finds instead.

use std::io::{self, Write as _};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::RunExit;
use super::leftovers::os::OsLeftovers;
use super::leftovers::{Kind, remove_all};
use super::spawn::{quiet_errors, restrict_dll_search};
use super::sweep::SweepError;
use super::sweep::os::end_sweep_own;
use super::sweeper_flow::{SweeperActs, run};
use crate::windows::acl::object::Handle;
use crate::windows::sids::Sid;
use crate::windows::token::os::{
    own_token, shut_sweeper, shut_token_first, user, widen_sweeper_for_com,
};

/// How long the whole sweep may take.
const SWEEP_LIMIT: Duration = Duration::from_secs(60);

/// The sweeper's whole run; its exit code.
pub fn run_sweep() -> u32 {
    if restrict_dll_search().is_err() {
        return RunExit::DllSearch.code();
    }
    quiet_errors();
    let (done, finished) = mpsc::channel();
    let worker = thread::Builder::new().spawn(move || {
        let _sent = done.send(sweep_all());
    });
    if let Err(error) = worker {
        say(&format!(
            "clauseo-sandbox-run: the end sweeper's thread failed ({})",
            error.kind()
        ));
        return RunExit::Sweep.code();
    }
    finished.recv_timeout(SWEEP_LIMIT).unwrap_or_else(|_| {
        say("clauseo-sandbox-run: the end sweeper ran out of time");
        RunExit::Sweep.code()
    })
}

/// The end sweeper's acts on Windows: its own token, opened with every right, and its user once
/// read.
struct OsActs {
    token: Handle,
    me: Option<Sid>,
}

impl SweeperActs for OsActs {
    fn shut(&mut self) -> Result<(), (&'static str, io::Error)> {
        // First of all, as the inner's first act: shut this process's own token to all but
        // SYSTEM.
        shut_token_first(&self.token).map_err(|e| ("shut its token", e))?;
        // Then its threads and later objects, the main thread blocked on this one included,
        // before any sweep or COM call starts one.
        shut_sweeper(&self.token).map_err(|e| ("shut its threads", e))?;
        self.me = Some(user(&self.token).map_err(|e| ("read its token's user", e))?);
        Ok(())
    }

    fn sweep(&mut self, log: &mut dyn FnMut(&str)) -> Result<(), SweepError> {
        let me = self.me().map_err(|e| SweepError::List(e.to_string()))?;
        end_sweep_own(me, log)
    }

    fn widen_for_com(&mut self) -> io::Result<()> {
        widen_sweeper_for_com(&self.token, self.me()?)
    }

    fn remove(&mut self, kinds: &[Kind], log: &mut dyn FnMut(&str)) -> bool {
        let Ok(me) = self.me().cloned() else {
            return false;
        };
        let mut calls = OsLeftovers::new(&self.token, me);
        remove_all(&mut calls, kinds, "end sweeper", log)
    }
}

impl OsActs {
    /// Its user, read by [`SweeperActs::shut`], which runs first.
    fn me(&self) -> io::Result<&Sid> {
        self.me
            .as_ref()
            .ok_or_else(|| io::Error::other("its user wasn't read"))
    }
}

/// Opens its token, then runs the acts in order (`sweeper_flow::run`); the exit code.
fn sweep_all() -> u32 {
    let token = match own_token() {
        Ok(token) => token,
        Err(error) => {
            let code = error
                .raw_os_error()
                .map_or_else(|| error.kind().to_string(), |c| c.to_string());
            say(&format!(
                "clauseo-sandbox-run: the end sweeper can't read its token ({code})"
            ));
            return RunExit::Sweep.code();
        }
    };
    let mut acts = OsActs { token, me: None };
    run(&mut acts, &mut |line| say(line))
}

/// One line on stderr, the pipe the outer reads while it lives.
fn say(line: &str) {
    let mut stderr = io::stderr().lock();
    drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
}
