//! The verify-only pass on a slot marked unclean (Windows), for `quarantine::admit`: `--verify`
//! started as the pool user with `CreateProcessWithLogonW`, suspended, its process and thread
//! shut as the end sweeper's are, then resumed and waited for up to [`VERIFY_WAIT`]. Its stdout
//! and stderr are a pipe whose lines go to this runner's stderr. It runs before anything of the
//! launch is made: no pipe, job, or the agent's window station and desktop names the pool user
//! yet. The pass gets a window station of its own (never `WinSta0`), which names no matter. A
//! previous session's runners are waited for and ended first (`previous.rs`).

use std::io::{BufRead as _, BufReader};
use std::thread;
use std::time::Duration;

use super::super::RunExit;
use super::super::desktop::AgentDesktop;
use super::super::marker::remove_marker;
use super::super::previous::clear;
use super::super::quarantine::{
    RESTART, Refused, Slot, admit, clear_unclean, ended, is_unclean, mark_unclean,
};
use super::super::spawn::{log_on, output_pipe, resume};
use super::super::station::StationSids;
use super::super::sweep::os::OsPrevious;
use super::end::OsSweeper;
use super::{Failure, os, say};
use crate::windows::secrets::secret_for;
use crate::windows::token::os::{shut_object, shut_process};
use crate::windows::token::{runner_process_sddl, runner_thread_sddl};

/// How long the pass has: its own 60 s limit (`sweeper.rs`), and its logon.
const VERIFY_WAIT: Duration = Duration::from_secs(75);

/// The slot of the end sweeper's pool user, whose start the pass shares.
pub(super) struct OsSlot<'a>(pub(super) &'a OsSweeper);

impl Slot for OsSlot<'_> {
    fn unclean(&mut self) -> bool {
        is_unclean(&self.0.data_dir, &self.0.pool_user)
    }

    fn verify(&mut self) -> Result<(), String> {
        let sweeper = self.0;
        // A previous session's runners (an end sweeper still running), which the pass can't
        // open, are waited for and ended first, as before the inner's logon (`previous.rs`).
        let image = std::path::Path::new(&sweeper.exe)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "the runner's file name".to_owned())?;
        let mut previous = OsPrevious {
            pool_user: &sweeper.pool_user,
            image,
        };
        clear(&mut previous).map_err(|(_, message)| message)?;
        let password =
            secret_for(&sweeper.data_dir, &sweeper.pool_user).map_err(|e| e.to_string())?;
        // Never `WinSta0`: a window station of its own, naming no matter, which lives until the
        // pass is waited for.
        let sids = StationSids {
            creator: &sweeper.desktop,
            pool_user: &sweeper.pool_user,
            matter: None,
        };
        let station = AgentDesktop::create(&sweeper.pool_name, &password, &sids)
            .map_err(|(call, e)| os(&format!("the verify pass's window station: {call}"), &e))?;
        let (read, write) = output_pipe().map_err(|e| os("CreatePipe", &e))?;
        // `log_on` takes the password and wipes it as it returns.
        let pass = log_on(
            &sweeper.pool_name,
            password,
            &sweeper.exe,
            "--verify",
            &sweeper.cwd,
            &station.path,
            Some(write.0),
        )
        .map_err(|code| format!("the verify pass's CreateProcessWithLogonW failed ({code})"))?;
        // Only the pass's copy of the write end is left, so its exit ends the reads.
        drop(write);
        let relay = thread::Builder::new().spawn(move || {
            for line in BufReader::new(read).lines() {
                match line {
                    Ok(line) => say(&line),
                    Err(_) => return,
                }
            }
        });
        if let Err(error) = relay {
            say(&format!(
                "clauseo-sandbox-run: {}",
                os("the verify pass's log", &error)
            ));
        }
        let shut = shut_process(pass.handle.0, &runner_process_sddl(&sweeper.desktop))
            .and_then(|()| shut_object(pass.thread.0, &runner_thread_sddl(&sweeper.desktop)))
            .map_err(|e| os("the verify pass's DACL", &e))
            .and_then(|()| resume(&pass).map_err(|e| os("ResumeThread", &e)));
        if let Err(error) = shut {
            pass.kill(RunExit::Sweep.code());
            return Err(error);
        }
        match pass.wait(Some(VERIFY_WAIT)) {
            Ok(Some(0)) => Ok(()),
            Ok(Some(code)) => Err(format!("the verify pass exited 0x{code:08X}")),
            Ok(None) => {
                pass.kill(RunExit::Sweep.code());
                Err("the verify pass didn't finish in time".to_owned())
            }
            Err(error) => {
                pass.kill(RunExit::Sweep.code());
                Err(os("the verify pass's wait", &error))
            }
        }
    }

    fn mark(&mut self) -> std::io::Result<()> {
        mark_unclean(&self.0.data_dir, &self.0.pool_user).map(drop)
    }

    fn clear(&mut self) -> std::io::Result<()> {
        clear_unclean(&self.0.data_dir, &self.0.pool_user)
    }
}

/// Verifies the slot if it is marked, then marks it (`quarantine::admit`): a pass that failed
/// refuses the launch with `RunExit::Quarantined` and the lawyer's first action ([`RESTART`]); a
/// mark that can't be written, with `RunExit::MarkerUnwritable`.
pub(super) fn admitted(slot: &mut OsSlot<'_>) -> Result<(), Failure> {
    admit(slot, &mut |line| say(line), || ()).map_err(|refused| match refused {
        Refused::Quarantined(why) => {
            say(&format!("clauseo-sandbox-run: {why}"));
            Failure::new(RunExit::Quarantined, RESTART)
        }
        Refused::Unmarked(why) => Failure::new(
            RunExit::MarkerUnwritable,
            format!("the unclean mark failed ({why})"),
        ),
    })
}

/// The session's end for its slot: a `clean` one removes the slot's mark (`quarantine::ended`)
/// and the session's marker (`marker.rs`); any other leaves both.
pub(super) fn ended_slot(sweeper: &OsSweeper, clean: bool) {
    ended(&mut OsSlot(sweeper), clean, &mut |line| say(line));
    if clean && let Err(error) = remove_marker(&sweeper.marker) {
        say(&format!(
            "clauseo-sandbox-run: {}",
            os("the session marker's removal", &error)
        ));
    }
}
