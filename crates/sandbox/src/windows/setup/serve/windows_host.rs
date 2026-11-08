//! The set-up task's [`Host`] on Windows: this process's token and image, the record, the
//! session's user, the identified user's profile and token, the set-up lock (a bounded wait), the
//! user's set-up state read as them, the set-up itself (`exe::run_set_up_in`), and the Event Log.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::windows_pipe::{WindowsPipe, ticks};
use super::{Gate, Host, ServeExit, TOKEN_BYTES, windows_published};
use crate::SetupState;
use crate::windows::admin_copy::{self, os};
use crate::windows::marker::setup_state;
use crate::windows::refresh::event_log;
use crate::windows::setup::accounts::{Kind, sid_of};
use crate::windows::setup::adopt::windows_adopter::own_profile;
use crate::windows::setup::exe::{owners_data_dir, run_set_up_in};
use crate::windows::setup::lock::{Held, SetupMutex};
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::outcome::SetupExit;
use crate::windows::setup::owner::{Owner, session_user, this_process};
use crate::windows::setup::record_write;
use crate::windows::setup::result_file::SetupResult;
use crate::windows::setup::windows_machine::Names;
use crate::windows::sids::Sid;

/// How long a request waits for the set-up lock, in milliseconds: a removal, a refresh, a stop or
/// a set-up started with a prompt may hold it; well inside each client's 30 minutes.
const LOCK_WAIT_MS: u32 = 300_000;

/// `--serve-set-up`: [`super::run`] over a [`WindowsHost`] and the production pipe.
pub(crate) fn run() -> ServeExit {
    let mut host = WindowsHost::new(Names::production());
    super::run(&mut host, WindowsPipe::create)
}

/// Why the record (or the group it names) couldn't be read, for the log and the reply: the
/// Windows call and its code, or a fixed line. Never the error's own words, which may hold the
/// registry's text as it was found.
fn unread(error: &MachineError) -> String {
    match error {
        MachineError::Os { call, code } => format!("{call} failed (error {code})"),
        MachineError::LockHeld
        | MachineError::Unsupported(_)
        | MachineError::Other(_)
        | MachineError::ProfileChanged(_) => "a value that isn't well formed".to_owned(),
    }
}

/// The machine the server reads and sets up, with the names `names`.
pub(crate) struct WindowsHost {
    names: Names,
    /// When the server started, by its clock (`windows_pipe::ticks`).
    started: Duration,
    /// The set-up lock, while held.
    held: Option<Held>,
}

impl WindowsHost {
    /// The host with the names `names` (`Names::production()` for a real task).
    pub(crate) fn new(names: Names) -> WindowsHost {
        WindowsHost {
            names,
            started: ticks(),
            held: None,
        }
    }
}

impl Host for WindowsHost {
    type Owner = Owner;

    fn is_system(&mut self) -> Result<bool, String> {
        this_process()
            .map(|(_elevated, system)| system)
            .map_err(|error| error.to_string())
    }

    fn current_exe(&mut self) -> Result<PathBuf, String> {
        os::running_image().map_err(|error| error.to_string())
    }

    fn checked_admin_copy(&mut self) -> Result<PathBuf, String> {
        admin_copy::check(&os::WindowsAdminCopy)
            .map_err(|blocked| format!("the admin copy fails its check ({})", blocked.name()))
    }

    fn gate(&mut self) -> Result<Gate, String> {
        let key = &self.names.record_key;
        let pool = record_write::read(key).map_err(|error| unread(&error))?;
        let (copy_owner, _version) =
            record_write::read_admin(key).map_err(|error| unread(&error))?;
        let mut groups: Vec<Sid> = pool.iter().map(|record| record.group.clone()).collect();
        if let Some(named) =
            sid_of(&self.names.group, Kind::Group).map_err(|error| unread(&error))?
            && !groups.contains(&named)
        {
            groups.push(named);
        }
        Ok(Gate {
            copy_owner,
            pool: pool.map(|record| record.pool).unwrap_or_default(),
            groups,
        })
    }

    fn session_user(&mut self, session: u32) -> Result<Sid, String> {
        session_user(session).map_err(|error| error.to_string())
    }

    fn profile(&mut self, user: &Sid) -> Result<Option<PathBuf>, String> {
        own_profile(user)
    }

    fn owner(&mut self, session: u32, user: &Sid) -> Result<Owner, String> {
        Owner::of_session(session, user).map_err(|error| error.to_string())
    }

    fn data_dir(&mut self, owner: &Owner, path: &Path) -> Result<PathBuf, (SetupExit, String)> {
        owners_data_dir(owner, path)
    }

    fn lock(&mut self) -> Result<bool, String> {
        let mutex = SetupMutex::open(&self.names.mutex).map_err(|error| error.to_string())?;
        match mutex.acquire(LOCK_WAIT_MS) {
            Ok((held, _acquired)) => {
                self.held = Some(held);
                Ok(true)
            }
            Err(MachineError::LockHeld) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    fn unlock(&mut self) {
        self.held = None;
    }

    fn current(&mut self, owner: &Owner, data_dir: &Path) -> bool {
        owner.run_as(|| setup_state(data_dir)) == Ok(SetupState::Ready)
    }

    fn set_up(&mut self, owner: Owner, data_dir: &Path, add: u8) -> SetupResult {
        // The set-up task's own run: it needs no prompt, so it doesn't register itself again.
        run_set_up_in(self.names.clone(), owner, data_dir, add, false)
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }

    fn elapsed(&mut self) -> Duration {
        ticks().saturating_sub(self.started)
    }

    fn random(&mut self) -> Result<[u8; TOKEN_BYTES], String> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
        Ok(bytes)
    }

    fn publish(&mut self, token: &str) -> Result<(), String> {
        windows_published::publish(&self.names.record_key, token).map_err(|error| unread(&error))
    }

    fn unpublish(&mut self, token: &str) {
        if let Err(error) = windows_published::unpublish(&self.names.record_key, token) {
            let line = format!(
                "--serve-set-up: clearing the pipe's name: {}",
                unread(&error)
            );
            event_log(true, &line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_is_logged_by_call_and_code_never_by_its_text() {
        let os = MachineError::Os {
            call: "RegGetValueW",
            code: 13,
        };
        assert_eq!(unread(&os), "RegGetValueW failed (error 13)");
        let found = MachineError::Other("not a SID: \"x\r\nforged line\"".to_owned());
        assert_eq!(unread(&found), "a value that isn't well formed");
    }
}
