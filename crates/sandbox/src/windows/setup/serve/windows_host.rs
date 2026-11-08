//! The set-up task's [`Host`] on Windows: this process's token and image, the record, the
//! session's user, the identified user's profile and token, the set-up lock (a bounded wait), the
//! user's set-up state read as them, the set-up itself (`exe::run_set_up_in`), and the Event Log.

use std::path::{Path, PathBuf};

use super::windows_pipe::WindowsPipe;
use super::{Gate, Host, PIPE_NAME, ServeExit};
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
/// a set-up started with a prompt may hold it; well inside the task's 30-minute limit.
const LOCK_WAIT_MS: u32 = 300_000;

/// `--serve-set-up`: [`super::run`] over a [`WindowsHost`] and the pipe [`PIPE_NAME`].
pub(crate) fn run() -> ServeExit {
    let mut host = WindowsHost::new(Names::production());
    super::run(&mut host, |sddl| WindowsPipe::create(PIPE_NAME, sddl))
}

/// The machine the server reads and sets up, with the names `names`.
pub(crate) struct WindowsHost {
    names: Names,
    /// The set-up lock, while held.
    held: Option<Held>,
}

impl WindowsHost {
    /// The host with the names `names` (`Names::production()` for a real task).
    pub(crate) fn new(names: Names) -> WindowsHost {
        WindowsHost { names, held: None }
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

    fn admin_copy(&mut self) -> Result<PathBuf, String> {
        let program_files = os::program_files().map_err(|error| error.to_string())?;
        Ok(admin_copy::admin_exe(&program_files))
    }

    fn gate(&mut self) -> Result<Gate, String> {
        let key = &self.names.record_key;
        let pool = record_write::read(key).map_err(|error| error.to_string())?;
        let (copy_owner, _version) =
            record_write::read_admin(key).map_err(|error| error.to_string())?;
        let mut groups: Vec<Sid> = pool.iter().map(|record| record.group.clone()).collect();
        if let Some(named) =
            sid_of(&self.names.group, Kind::Group).map_err(|error| error.to_string())?
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
        run_set_up_in(self.names.clone(), owner, data_dir, add)
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}
