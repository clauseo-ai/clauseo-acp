//! The detached end sweeper (Windows): `clauseo-sandbox-run --sweep`, started by the outer as
//! the pool user at a session's end (`end.rs`), outside the launch's job. It takes nothing but
//! its flag. It kills every process of its own user but itself (no job is kept: the session is
//! ending) that was created before it was: a next session's processes, started later, are left
//! alone (`sweep.rs`, which also says why a next inner killing this sweeper loses nothing). It
//! then removes what that user could leave to start at its next logon (`leftovers.rs`), all on
//! its own [`SWEEP_LIMIT`]. Its stderr gets each kill (pid, image name,
//! sweep point) and each kind's count, which the outer relays while it lives. It exits 0 when
//! everything went, [`RunExit::Sweep`] when anything failed or the time ran out.
//!
//! It writes no result file: the run folder admits only the desktop user, `SYSTEM` and
//! `Administrators`, and a file the pool user could write there would be a path its other
//! processes could write too. The next launch's first-act cleanup logs what it finds instead.

use std::io::{self, Write as _};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::RunExit;
use super::inner_flow::at;
use super::leftovers::os::OsLeftovers;
use super::leftovers::{Kind, remove_all};
use super::spawn::{quiet_errors, restrict_dll_search};
use super::sweep::Point;
use super::sweep::os::sweep_own;
use crate::windows::token::os::{own_token, user};

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
    match finished.recv_timeout(SWEEP_LIMIT) {
        Ok(true) => 0,
        Ok(false) => RunExit::Sweep.code(),
        Err(_) => {
            say("clauseo-sandbox-run: the end sweeper ran out of time");
            RunExit::Sweep.code()
        }
    }
}

/// The kills, then the removal; whether both went without a failure.
fn sweep_all() -> bool {
    let token = match own_token() {
        Ok(token) => token,
        Err(error) => return failed("its token", &error),
    };
    let me = match user(&token) {
        Ok(me) => me,
        Err(error) => return failed("its token's user", &error),
    };
    let mut log = |line: &str| say(line);
    let swept = match sweep_own(&me, None, Point::Sweeper, &mut log) {
        Ok(()) => true,
        Err(error) => {
            say(&format!(
                "clauseo-sandbox-run: {}",
                at(Point::Sweeper, &error)
            ));
            false
        }
    };
    let mut calls = OsLeftovers::new(&token, me);
    let clean = remove_all(&mut calls, &Kind::ALL, "end sweeper", &mut log);
    swept && clean
}

fn failed(what: &str, error: &io::Error) -> bool {
    let code = error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string());
    say(&format!(
        "clauseo-sandbox-run: the end sweeper can't read {what} ({code})"
    ));
    false
}

/// One line on stderr, the pipe the outer reads while it lives.
fn say(line: &str) {
    let mut stderr = io::stderr().lock();
    drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
}
