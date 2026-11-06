//! [`WindowsMachine`]: the real [`Machine`]. S15 `PR4e` built the account, group, hidden-user,
//! record and lock methods; `PR4f`'s logon, profile, preflight and journal calls and `PR4g`'s
//! `secrets.bin` are wired in by `PR4h`, with the owner's files (`setup.json`, the ledger, the
//! open sessions) and the `ProgramData` known folder.
//!
//! Every call on the owner's files runs as the owner ([`Machine::as_owner`], `Owner::run_as`):
//! the engine asks for it, and each such method here also takes it itself when called outside,
//! so no path writes under `<data_dir>\sandbox\` as the elevated process's own user.
//!
//! The lock and every call run on the thread that made the machine: the set-up mutex belongs to
//! the thread that acquired it, and impersonation is the thread's, so `WindowsMachine` is not
//! `Send`.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::accounts::{self, Kind};
use super::lock::{Acquired, Held, SETUP_MUTEX, SetupMutex};
use super::machine::{
    LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart, owned,
};
use super::owner::Owner;
use super::record_write::{self, RECORD_KEY};
use super::{ffi, journal, logon, password, preflight, profile};
use crate::SetupError;
use crate::windows::apply::Surroundings;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::marker::{self, Marker, MarkerState};
use crate::windows::matters::MatterSids;
use crate::windows::pool::{MachineRecord, POOL_GROUP, POOL_PREFIX, pool_number};
use crate::windows::secrets;
use crate::windows::sids::{Reserved, Sid};

/// The comment the pool group carries in Computer Management.
const GROUP_COMMENT: &str = "Clauseo's protection runs each matter's agent as one of these users.";

/// The machine-wide names a [`WindowsMachine`] works on, and what it may touch: the real ones
/// ([`Names::production`]), or a test's, so a test never touches a real pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// The record's key under `HKLM`: `SOFTWARE\Clauseo\Sandbox`.
    pub record_key: String,
    /// The pool group: `ClauseoSandboxUsers`.
    pub group: String,
    /// The set-up lock's mutex, named inside the set-up's private namespace: `SetupLock`.
    pub mutex: String,
    /// What stands for [`POOL_PREFIX`] in the accounts' real names: `ClauseoSb` itself, or a
    /// test's prefix, so the engine's `ClauseoSbNN` is made as `<users>NN`.
    pub users: String,
    /// Whether the change journals may be enlarged; a test's machine leaves them as they are.
    pub journals: bool,
}

impl Names {
    /// The names a real set-up uses.
    pub fn production() -> Names {
        Names {
            record_key: RECORD_KEY.to_owned(),
            group: POOL_GROUP.to_owned(),
            mutex: SETUP_MUTEX.to_owned(),
            users: POOL_PREFIX.to_owned(),
            journals: true,
        }
    }

    /// The real account name of the engine's `name`: a pool name with [`Names::users`] for
    /// its prefix; any other name as it is.
    fn real(&self, name: &str) -> String {
        match (pool_number(name), name.strip_prefix(POOL_PREFIX)) {
            (Some(_), Some(digits)) => format!("{}{digits}", self.users),
            _ => name.to_owned(),
        }
    }

    /// The engine's name for the real account name `real`, the inverse of [`Names::real`].
    fn engine(&self, real: String) -> String {
        match real.strip_prefix(self.users.as_str()) {
            Some(digits) if pool_number(&format!("{POOL_PREFIX}{digits}")).is_some() => {
                format!("{POOL_PREFIX}{digits}")
            }
            _ => real,
        }
    }
}

/// This Windows machine, as the set-up engine ([`set_up`](super::set_up)) drives it.
pub struct WindowsMachine {
    names: Names,
    /// The set-up lock while held.
    held: Option<Held>,
    /// What the fence's walks need from around the matters; the setup exe gives it.
    around: Box<dyn Surroundings>,
    /// The user who asked (D-B2), whose files the set-up writes as that user.
    owner: Rc<Owner>,
    /// The owner's data folder, which holds `sandbox\`.
    data_dir: PathBuf,
    /// Whether this thread acts as the owner now (inside [`Machine::as_owner`]).
    acting: Rc<Cell<bool>>,
}

impl WindowsMachine {
    /// A machine working on `names` for `owner`, whose data folder is `data_dir`, with
    /// `around` for the fence's walks.
    pub fn new(
        names: Names,
        owner: Owner,
        data_dir: PathBuf,
        around: Box<dyn Surroundings>,
    ) -> WindowsMachine {
        WindowsMachine {
            names,
            held: None,
            around,
            owner: Rc::new(owner),
            data_dir,
            acting: Rc::new(Cell::new(false)),
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
            .field("owner", self.owner.sid())
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

/// The thread acting as the owner: set while it lives, cleared on drop (a panic's included).
struct Acting(Rc<Cell<bool>>);

impl Acting {
    fn set(flag: &Rc<Cell<bool>>) -> Acting {
        flag.set(true);
        Acting(Rc::clone(flag))
    }
}

impl Drop for Acting {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// A set-up error's words, as a machine error.
fn from_setup(error: SetupError) -> MachineError {
    match error {
        SetupError::Failed(why) => MachineError::Other(why),
        SetupError::Refused => MachineError::Other(error.to_string()),
    }
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
        preflight::seclogon()
    }

    fn logon_right(&mut self) -> Result<LogonRight, MachineError> {
        preflight::logon_right()
    }

    fn account_sid(&mut self, name: &str) -> Result<Option<Sid>, MachineError> {
        accounts::sid_of(&self.names.real(name), Kind::User)
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError> {
        Ok(accounts::name_of(sid, Kind::User)?.map(|real| self.names.engine(real)))
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
        accounts::create_user(&self.names.real(name), password)
    }

    fn set_password(&mut self, name: &str, password: &Password) -> Result<(), MachineError> {
        accounts::set_password(&self.names.real(name), password)
    }

    fn add_to_group(&mut self, name: &str, group: &Sid) -> Result<(), MachineError> {
        let group_name = self.group_name(group)?;
        let name = self.names.real(name);
        let member = accounts::sid_of(&name, Kind::User)?
            .ok_or_else(|| MachineError::Other(format!("no local account {name}")))?;
        accounts::add_member(&group_name, &member)?;
        accounts::add_member(&accounts::users_group()?, &member)
    }

    fn hide(&mut self, name: &str) -> Result<(), MachineError> {
        accounts::hide(&self.names.real(name))
    }

    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        accounts::delete_user(&self.names.real(name))
    }

    fn log_on_once(&mut self, name: &str, password: &Password) -> Result<ProfileDir, MachineError> {
        logon::log_on_once(&self.names.real(name), password)
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
        profile::profile_of(sid)
    }

    fn delete_profile(&mut self, sid: &Sid) -> Result<(), MachineError> {
        profile::delete_profile(sid, self.owner.sid())
    }

    fn program_data(&mut self) -> Result<PathBuf, MachineError> {
        ffi::program_data()
    }

    fn read_marker(&mut self) -> MarkerState {
        self.as_owner(|m| marker::read(&m.data_dir, m.owner.sid()))
            .unwrap_or_else(|error| MarkerState::Unreadable(error.to_string()))
    }

    fn write_marker(&mut self, marker: &Marker) -> Result<(), MachineError> {
        owned(self, |m| {
            marker::write(&m.data_dir, marker).map_err(from_setup)
        })
    }

    fn sessions_open(&mut self) -> Result<bool, MachineError> {
        owned(self, |m| {
            let open = MatterSids::load(&m.data_dir)
                .and_then(|matters| matters.open_sessions())
                .map_err(from_setup)?;
            Ok(!open.is_empty())
        })
    }

    fn ledger(&mut self, trusted: &[Sid]) -> io::Result<Ledger> {
        let opened = self.as_owner(|m| {
            // As the owner, so this machine's reservations name the owner, not the admin.
            let reserved = Reserved::of_this_machine().map_err(io::Error::other)?;
            Ledger::open(ledger_path(&m.data_dir), trusted, &reserved)
        });
        opened.unwrap_or_else(|error| Err(io::Error::other(error)))
    }

    fn surroundings(&mut self) -> &mut dyn Surroundings {
        &mut *self.around
    }

    fn journal_volumes(&mut self, roots: &[&Path]) -> Result<Vec<PathBuf>, MachineError> {
        if !self.names.journals {
            return Ok(Vec::new());
        }
        journal::journal_volumes(roots)
    }

    fn journal_size(&mut self, volume: &Path) -> Result<Option<u64>, MachineError> {
        journal::journal_size(volume)
    }

    fn grow_journal(&mut self, volume: &Path, max: u64, delta: u64) -> Result<(), MachineError> {
        if !self.names.journals {
            return Err(MachineError::Other(
                "a test's machine grows no journal".to_owned(),
            ));
        }
        journal::grow_journal(volume, max, delta)
    }

    fn protect_for_owner(
        &mut self,
        secrets: &[(Sid, Password)],
        pool: &[Sid],
    ) -> Result<(), MachineError> {
        owned(self, |m| {
            secrets::protect_as_this_thread(&m.data_dir, secrets, pool)
        })
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

    fn owner(&self) -> &Sid {
        self.owner.sid()
    }

    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError> {
        if self.acting.get() {
            return Ok(f(self));
        }
        let owner = Rc::clone(&self.owner);
        let _as_owner = owner.impersonate()?;
        // Dropped first: the flag clears, then the thread reverts.
        let _acting = Acting::set(&self.acting);
        Ok(f(self))
    }
}
