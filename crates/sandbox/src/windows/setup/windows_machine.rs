//! [`WindowsMachine`]: the real [`Machine`]. S15 `PR4e` builds the account, group, hidden-user,
//! record and lock methods; the rest answer [`MachineError::Unsupported`] (or the nearest
//! "not built" answer their type allows) until `PR4f` (logon, profiles, preflight, journal),
//! `PR4g` (`secrets.bin` as the owner) and `PR4h` (the owner's files) wire theirs in.
//!
//! The lock and every call run on the thread that made the machine: the set-up mutex belongs to
//! the thread that acquired it, so `WindowsMachine` is not `Send`.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::accounts::{self, Kind};
use super::lock::{Acquired, Held, SETUP_MUTEX, SetupMutex};
use super::machine::{LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart};
use super::password;
use super::record_write::{self, RECORD_KEY};
use crate::windows::apply::Surroundings;
use crate::windows::ledger::Ledger;
use crate::windows::marker::{Marker, MarkerState};
use crate::windows::pool::{MachineRecord, POOL_GROUP};
use crate::windows::sids::Sid;

/// The comment the pool group carries in Computer Management.
const GROUP_COMMENT: &str = "Clauseo's protection runs each matter's agent as one of these users.";

/// The machine-wide names a [`WindowsMachine`] works on: the real ones
/// ([`Names::production`]), or a test's, so a test never touches a real pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// The record's key under `HKLM`: `SOFTWARE\Clauseo\Sandbox`.
    pub record_key: String,
    /// The pool group: `ClauseoSandboxUsers`.
    pub group: String,
    /// The set-up lock's mutex, named inside the set-up's private namespace: `SetupLock`.
    pub mutex: String,
}

impl Names {
    /// The names a real set-up uses.
    pub fn production() -> Names {
        Names {
            record_key: RECORD_KEY.to_owned(),
            group: POOL_GROUP.to_owned(),
            mutex: SETUP_MUTEX.to_owned(),
        }
    }
}

/// This Windows machine, as the set-up engine ([`set_up`](super::set_up)) drives it.
pub struct WindowsMachine {
    names: Names,
    /// The set-up lock while held.
    held: Option<Held>,
    /// What the fence's walks need from around the matters; the setup exe (`PR4h`) gives it.
    around: Box<dyn Surroundings>,
}

impl WindowsMachine {
    /// A machine working on `names`, with `around` for the fence's walks.
    pub fn new(names: Names, around: Box<dyn Surroundings>) -> WindowsMachine {
        WindowsMachine {
            names,
            held: None,
            around,
        }
    }

    /// The group's name, once `group` is checked to be the pool group's SID: the account calls
    /// take names, and a SID that isn't this machine's pool group is never acted on.
    fn group_name(&self, group: &Sid) -> Result<String, MachineError> {
        match accounts::sid_of(&self.names.group, Kind::Group)? {
            Some(found) if &found == group => Ok(self.names.group.clone()),
            _ => Err(MachineError::Other(format!(
                "{group} is not the group {}",
                self.names.group
            ))),
        }
    }
}

impl std::fmt::Debug for WindowsMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsMachine")
            .field("names", &self.names)
            .field("locked", &self.held.is_some())
            .finish_non_exhaustive()
    }
}

/// The answer of a method a later PR builds.
fn unbuilt<T>(method: &'static str) -> Result<T, MachineError> {
    Err(MachineError::Unsupported(method))
}

impl Machine for WindowsMachine {
    fn lock(&mut self) -> Result<(), MachineError> {
        // Held already: a second `lock` without its `unlock` is a caller's bug, refused.
        if self.held.is_some() {
            return Err(MachineError::LockHeld);
        }
        let (held, acquired) = SetupMutex::open(&self.names.mutex)?.acquire(0)?;
        if acquired == Acquired::Abandoned {
            tracing::warn!("an earlier set-up ended without giving its lock back; taking it");
        }
        self.held = Some(held);
        Ok(())
    }

    fn unlock(&mut self) {
        self.held = None;
    }

    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        record_write::read(&self.names.record_key)
    }

    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError> {
        record_write::write(&self.names.record_key, record)
    }

    fn delete_record(&mut self) -> Result<(), MachineError> {
        record_write::delete(&self.names.record_key)
    }

    fn seclogon(&mut self) -> Result<ServiceStart, MachineError> {
        unbuilt("seclogon")
    }

    fn logon_right(&mut self) -> Result<LogonRight, MachineError> {
        unbuilt("logon_right")
    }

    fn account_sid(&mut self, name: &str) -> Result<Option<Sid>, MachineError> {
        accounts::sid_of(name, Kind::User)
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError> {
        accounts::name_of(sid, Kind::User)
    }

    fn group(&mut self) -> Result<Option<Sid>, MachineError> {
        accounts::sid_of(&self.names.group, Kind::Group)
    }

    fn create_group(&mut self) -> Result<Sid, MachineError> {
        accounts::create_group(&self.names.group, GROUP_COMMENT)
    }

    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError> {
        accounts::group_members(&self.group_name(group)?)
    }

    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        accounts::delete_group(&self.group_name(group)?)
    }

    fn new_password(&mut self) -> Password {
        password::generate().unwrap_or_else(|| {
            // The account calls refuse an empty password, so this fails the step that uses it.
            tracing::error!("the OS's random source failed; no password made");
            Password::new(String::new())
        })
    }

    fn create_user(&mut self, name: &str, password: &Password) -> Result<Sid, MachineError> {
        accounts::create_user(name, password)
    }

    fn set_password(&mut self, name: &str, password: &Password) -> Result<(), MachineError> {
        accounts::set_password(name, password)
    }

    fn add_to_group(&mut self, name: &str, group: &Sid) -> Result<(), MachineError> {
        let group_name = self.group_name(group)?;
        let member = accounts::sid_of(name, Kind::User)?
            .ok_or_else(|| MachineError::Other(format!("no local account {name}")))?;
        accounts::add_member(&group_name, &member)?;
        accounts::add_member(&accounts::users_group()?, &member)
    }

    fn hide(&mut self, name: &str) -> Result<(), MachineError> {
        accounts::hide(name)
    }

    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        accounts::delete_user(name)
    }

    fn log_on_once(
        &mut self,
        _name: &str,
        _password: &Password,
    ) -> Result<ProfileDir, MachineError> {
        unbuilt("log_on_once")
    }

    fn profile_of(&mut self, _sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
        unbuilt("profile_of")
    }

    fn delete_profile(&mut self, _sid: &Sid) -> Result<(), MachineError> {
        unbuilt("delete_profile")
    }

    fn program_data(&mut self) -> Result<PathBuf, MachineError> {
        unbuilt("program_data")
    }

    fn read_marker(&mut self) -> MarkerState {
        MarkerState::Unreadable("reading setup.json as the owner isn't built yet".to_owned())
    }

    fn write_marker(&mut self, _marker: &Marker) -> Result<(), MachineError> {
        unbuilt("write_marker")
    }

    fn sessions_open(&mut self) -> Result<bool, MachineError> {
        unbuilt("sessions_open")
    }

    fn ledger(&mut self, _trusted: &[Sid]) -> io::Result<Ledger> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "opening the owner's ledger isn't built yet",
        ))
    }

    fn surroundings(&mut self) -> &mut dyn Surroundings {
        &mut *self.around
    }

    fn journal_volumes(&mut self, _roots: &[&Path]) -> Result<Vec<PathBuf>, MachineError> {
        unbuilt("journal_volumes")
    }

    fn journal_size(&mut self, _volume: &Path) -> Result<Option<u64>, MachineError> {
        unbuilt("journal_size")
    }

    fn grow_journal(&mut self, _volume: &Path, _max: u64, _delta: u64) -> Result<(), MachineError> {
        unbuilt("grow_journal")
    }

    fn protect_for_owner(
        &mut self,
        _secrets: &[(Sid, Password)],
        _pool: &[Sid],
    ) -> Result<(), MachineError> {
        unbuilt("protect_for_owner")
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the marker's set_up_at, the set-up's one clock; the engine takes it from here"
    )]
    fn now(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
    }
}
