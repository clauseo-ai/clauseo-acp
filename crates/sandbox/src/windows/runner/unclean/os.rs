//! The markers' effects on Windows (`unclean.rs`), as the lawyer: the run folder's markers, the
//! record's sequences and `PoolSids` (the record grants Users read), and requests for the stop
//! and sweep tasks through the owner's `FRFX` entry. [`start_up_requests`] is the desktop's call.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::super::marker::{
    MarkerState, Sequences, UncleanSession, marker_path, remove_marker, unclean_sessions,
    write_marker_at,
};
use super::{Markers, Refused, StartUp, await_sweeps, logon_check, start_up};
use crate::windows::record::sandbox_pool_sids;
use crate::windows::refresh::{request_pool_stop, request_pool_sweep, tasks_repair};
use crate::windows::setup::record_values::{STOP_SEQUENCE, SWEEP_SEQUENCE};
use crate::windows::setup::{read_record_install_id, read_record_value};
use crate::windows::sids::Sid;

/// The markers of the data folder `data_dir`.
pub(crate) struct OsMarkers {
    data_dir: PathBuf,
}

impl OsMarkers {
    pub(crate) fn new(data_dir: &Path) -> OsMarkers {
        OsMarkers {
            data_dir: data_dir.to_path_buf(),
        }
    }
}

/// A record value as the lawyer reads it, `absent` when it is missing.
fn value(name: &str, absent: u64) -> Result<u64, String> {
    let read = read_record_value(name).map_err(|error| error.to_string())?;
    Ok(read.unwrap_or(absent))
}

impl Markers for OsMarkers {
    fn sessions(&mut self) -> io::Result<Vec<UncleanSession>> {
        unclean_sessions(&self.data_dir)
    }

    fn sequences(&mut self) -> Result<Sequences, String> {
        Ok(Sequences {
            stop: value(STOP_SEQUENCE, 0)?,
            sweep: value(SWEEP_SEQUENCE, 0)?,
        })
    }

    fn next_number(&mut self, name: &'static str) -> Result<u64, String> {
        value(name, 1)
    }

    fn pool(&mut self) -> Vec<Sid> {
        sandbox_pool_sids().unwrap_or_default()
    }

    fn install_id(&mut self) -> Result<Option<String>, String> {
        read_record_install_id().map_err(|error| error.to_string())
    }

    fn settle(
        &mut self,
        id: &str,
        pool_user: Option<&Sid>,
        install: Option<&str>,
        state: MarkerState,
    ) -> io::Result<()> {
        write_marker_at(&marker_path(&self.data_dir, id), pool_user, install, state)
    }

    fn remove(&mut self, id: &str) -> io::Result<()> {
        remove_marker(&marker_path(&self.data_dir, id))
    }

    fn request_stop(&mut self) -> bool {
        request_pool_stop()
    }

    fn request_sweep(&mut self) -> Result<(), String> {
        request_pool_sweep().map_err(|error| error.to_string())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a launch's bounded wait for a sweep a marker awaits, 100 ms at a time; the \
                  tests inject the pause (`Markers::pause`)"
    )]
    fn pause(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }

    fn note(&mut self, line: &str) {
        let mut stderr = io::stderr().lock();
        drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
    }

    fn task_repair(&mut self) -> Option<&'static str> {
        tasks_repair()
    }
}

/// F4, for the outer before it writes its own marker (`unclean::logon_check`).
pub(crate) fn check_logon(data_dir: &Path, pool_user: &Sid) -> Result<(), Refused> {
    logon_check(&mut OsMarkers::new(data_dir), pool_user)
}

/// A launch's check, for `clean_close::check_launch` (`unclean::await_sweeps`).
pub(crate) fn check_sweeps(data_dir: &Path) -> Result<(), String> {
    await_sweeps(&mut OsMarkers::new(data_dir))
}

/// The desktop's call at its start, once, before any launch and off the UI thread (it makes
/// COM calls and reads the registry): every marker a dead outer left becomes one awaiting a stop,
/// the clean and stale ones go, and `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep` are
/// requested for what is still awaited. It never waits for a run: the next launch and logon
/// check the markers again (`unclean.rs`). `data_dir` is the app's data folder. An error is the
/// run folder that couldn't be listed.
pub fn start_up_requests(data_dir: &Path) -> io::Result<StartUp> {
    start_up(&mut OsMarkers::new(data_dir))
}
