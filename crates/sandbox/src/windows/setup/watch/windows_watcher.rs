//! The watcher's real [`Watcher`]: the record in `HKLM\SOFTWARE\Clauseo\Sandbox`, the admin
//! copy's path, the trace (`windows_trace.rs`), the pipe (`windows_pipe.rs`) and the Event Log.
#![expect(unsafe_code, reason = "GetTickCount64, the monotonic clock")]

use std::path::PathBuf;
use std::time::Duration;

use windows::Win32::System::SystemInformation::GetTickCount64;

use super::windows_pipe::Server;
use super::windows_trace::Trace;
use super::{Delivery, Failed, Pool, TraceError, WatchExit, Watcher, watch};
use crate::windows::admin_copy::{self, os};
use crate::windows::refresh::event_log;
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::owner::this_process;
use crate::windows::setup::processes::check_pool;
use crate::windows::setup::record_write::{self, RECORD_KEY};
use crate::windows::sids::Sid;

/// `--watch`: [`watch`] over a [`WindowsWatcher`].
pub(crate) fn run() -> WatchExit {
    watch(&mut WindowsWatcher {
        // SAFETY: GetTickCount64 only reads the tick count.
        began_ms: unsafe { GetTickCount64() },
        trace: None,
        server: None,
    })
}

/// The machine the watcher acts on.
pub(crate) struct WindowsWatcher {
    /// `GetTickCount64` when the watcher began.
    began_ms: u64,
    trace: Option<Trace>,
    server: Option<Server>,
}

impl Watcher for WindowsWatcher {
    fn is_system(&mut self) -> Result<bool, Failed> {
        this_process()
            .map(|(_elevated, system)| system)
            .map_err(|error| error.to_string())
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        os::running_image().map_err(|error| error.to_string())
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        let program_files = os::program_files().map_err(|error| error.to_string())?;
        Ok(admin_copy::admin_exe(&program_files))
    }

    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        let record = record_write::read(RECORD_KEY).map_err(|error| error.to_string())?;
        Ok(record.map(|record| Pool {
            owner: record.owner,
            users: record.pool,
        }))
    }

    fn check_pool(&mut self, pool: &Pool) -> Result<(), Failed> {
        // The check's own refusal names the SID; the Application log is readable by every
        // local user, so the line says only that one failed.
        check_pool(&pool.users, &pool.owner).map_err(|error| match error {
            MachineError::Other(_) => "a SID PoolSids lists failed the pool-user check".to_owned(),
            error @ (MachineError::Os { .. }
            | MachineError::LockHeld
            | MachineError::Unsupported(_)
            | MachineError::ProfileChanged(_)) => error.to_string(),
        })
    }

    fn start_trace(&mut self, pool: &[Sid]) -> Result<(), TraceError> {
        self.trace = Some(Trace::start(pool)?);
        Ok(())
    }

    fn stop_trace(&mut self) {
        if let Some(trace) = self.trace.take()
            && let Err(error) = trace.stop()
        {
            event_log(
                true,
                &format!("the sandbox's watcher: its session: {error}"),
            );
        }
    }

    fn open_pipe(&mut self, owner: &Sid) -> Result<(), Failed> {
        let server = Server::open(owner).map_err(|error| format!("the pipe: {error}"))?;
        self.server = Some(server);
        Ok(())
    }

    fn close_pipe(&mut self) {
        if let Some(server) = self.server.take() {
            server.close();
        }
    }

    fn records(&mut self, wait: Duration) -> Delivery {
        match &mut self.trace {
            Some(trace) => trace.records(wait),
            None => Delivery::Ended,
        }
    }

    fn clients(&mut self) -> Result<usize, Failed> {
        self.server
            .as_mut()
            .map(Server::poll)
            .ok_or_else(|| "the pipe isn't open".to_owned())
    }

    fn send(&mut self, line: &str) {
        if let Some(server) = &mut self.server {
            server.send(line.as_bytes());
        }
    }

    fn now(&mut self) -> Duration {
        // SAFETY: GetTickCount64 only reads the tick count.
        Duration::from_millis(unsafe { GetTickCount64() }.saturating_sub(self.began_ms))
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}
