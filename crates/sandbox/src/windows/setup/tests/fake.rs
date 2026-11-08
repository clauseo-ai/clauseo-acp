//! The set-up's seams faked in memory: [`FakeMachine`] records every call as a line of text and
//! fails the calls it is told to; [`FakeAcl`] (`fake_acl.rs`) keeps the entries written, and
//! `fake_more.rs` holds the surroundings, the call lines' folder names, and the machine's
//! helpers outside the trait impl (its constructor and call recording).
//! Profiles and `DeviceSync` are real folders in a temp folder, since the shared-write apply
//! checks they exist, and the ledger is a real file there.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::test_support::TempDir;
use crate::windows::acl_plan::Sid;
use crate::windows::apply::Surroundings;
use crate::windows::fence::RegisteredMatter;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::marker::{Marker, MarkerState};
use crate::windows::pool::MachineRecord;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::setup::{
    LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart, machine::GroupFacts,
};
use crate::windows::sids::Reserved;
use crate::windows::wfp::{NetworkReport, NetworkSetup};

pub(super) use super::fake_acl::FakeAcl;
pub(super) use super::fake_more::folder_name;
pub(super) use super::fake_more::{Account, State};
use super::fake_more::{Around, ProfileFaults};

/// The owner's SID, and its account's name.
pub(super) const OWNER: &str = "S-1-5-21-7-8-9-1001";
pub(super) const OWNER_NAME: &str = "Rohan";
/// Another Windows user.
pub(super) const OTHER_OWNER: &str = "S-1-5-21-7-8-9-1002";
/// The volume the fake lists for the change journal.
pub(super) const VOLUME: &str = "C:";

/// `SYSTEM` and `BUILTIN\Administrators`.
pub(super) const SYSTEM: &str = "S-1-5-18";
pub(super) const ADMINISTRATORS: &str = "S-1-5-32-544";

/// The SID the fake gives the account or group with relative id `rid`.
pub(super) fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-5-5-5-{rid}")).unwrap()
}

/// `sid`'s last part, as the call lines write it.
pub(super) fn rid(sid: &Sid) -> String {
    sid.as_str().rsplit('-').next().unwrap().to_owned()
}

/// SIDs as their relative ids, joined with commas.
pub(super) fn rids(sids: &[Sid]) -> String {
    sids.iter().map(rid).collect::<Vec<_>>().join(",")
}

/// Whose set-up a fake machine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Run {
    /// An elevated set-up: the prompt's `--set-up`, or `--adopt`'s.
    Elevated,
    /// The set-up task's own run.
    Task,
}

/// The machine, in memory.
pub(super) struct FakeMachine {
    pub(super) dir: TempDir,
    /// Every call, as a line: the method and what it names.
    pub(super) calls: Vec<String>,
    /// Each fails the first call whose line it is, once, with its error.
    pub(super) fails: Vec<(String, MachineError)>,
    /// The call whose line this is panics.
    pub(super) panic_on: Option<String>,
    pub(super) state: State,
    /// Every record written, in order.
    pub(super) written: Vec<MachineRecord>,
    /// Accounts whose logon gets a temporary profile.
    pub(super) temporary: BTreeSet<String>,
    pub(super) seclogon: ServiceStart,
    pub(super) logon_right: LogonRight,
    pub(super) lock_held: bool,
    pub(super) sessions_open: bool,
    pub(super) journals: BTreeMap<PathBuf, Option<u64>>,
    /// `create_user` of this name answers the owner's SID.
    pub(super) owner_sid_for: Option<String>,
    /// The owner the machine acts as ([`OWNER`] unless a test says otherwise).
    pub(super) owner: Sid,
    /// Whether the calling thread acts as the owner now: inside `as_owner`. A [`FakeAcl`]
    /// made with [`FakeAcl::of`] shares it, so its writes say whose they were.
    pub(super) acting: Rc<Cell<bool>>,
    /// Every call on the owner's files made while not acting as the owner: none, ever.
    pub(super) outside_owner: Vec<String>,
    /// `as_owner` fails, as an impersonation Windows refuses would.
    pub(super) refuse_owner: bool,
    /// Whose set-up this is: an elevated one registers the set-up task again; the set-up task's
    /// own run doesn't.
    pub(super) run: Run,
    /// The owner's registered matters, as `registered_matters` reads them.
    pub(super) matters: Vec<RegisteredMatter>,
    /// Each folder's owner; one not listed is `SYSTEM`'s.
    pub(super) owners: BTreeMap<PathBuf, Sid>,
    /// Accounts whose first logon leaves their `LocalLow` owned by the account itself.
    pub(super) self_owned_local_low: BTreeSet<String>,
    /// The folders (by their call-line name) whose `give_to_admins` succeeds but leaves the
    /// owner as it was.
    pub(super) stuck_owners: BTreeSet<String>,
    /// Accounts whose profile has a reparse point on the way to `LocalLow` (`AppData`):
    /// `in_profile` and `give_to_admins` refuse it as `ProfileChanged`.
    pub(super) changed_profiles: BTreeSet<String>,
    /// Accounts whose logon leaves their profile with no `LocalLow`.
    pub(super) no_local_low: BTreeSet<String>,
    /// The users the owner's `matters.json` marks as owed a profile reset.
    pub(super) needs_reset: BTreeSet<Sid>,
    /// The users the record's `ResetSids` lists (the stop task's marks).
    pub(super) stop_resets: BTreeSet<Sid>,
    /// The users the reuse rule holds back after the resets, as `held_users` reads them.
    pub(super) held: BTreeSet<Sid>,
    /// The `resets` each `held_users` call was given.
    pub(super) held_for: Vec<Vec<Sid>>,
    pub(super) faults: ProfileFaults,
    pub(super) next_rid: u32,
    pub(super) passwords: u32,
    pub(super) around: Around,
}

impl Machine for FakeMachine {
    fn lock(&mut self) -> Result<(), MachineError> {
        self.call("lock")?;
        if self.lock_held {
            return Err(MachineError::LockHeld);
        }
        Ok(())
    }

    fn unlock(&mut self) {
        self.calls.push("unlock".to_owned());
    }

    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        self.call("read_record")?;
        Ok(self.state.record.clone())
    }

    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError> {
        self.call(format!("write_record {}", rids(&record.pool)))?;
        self.state.record = Some(record.clone());
        self.written.push(record.clone());
        Ok(())
    }

    fn delete_record(&mut self) -> Result<(), MachineError> {
        self.call("delete_record")?;
        self.state.record = None;
        Ok(())
    }

    fn seclogon(&mut self) -> Result<ServiceStart, MachineError> {
        self.call("seclogon")?;
        Ok(self.seclogon)
    }

    fn logon_right(&mut self) -> Result<LogonRight, MachineError> {
        self.call("logon_right")?;
        Ok(self.logon_right)
    }

    fn account_sid(&mut self, name: &str) -> Result<Option<Sid>, MachineError> {
        self.call(format!("account_sid {name}"))?;
        Ok(self
            .state
            .accounts
            .get(name)
            .map(|account| account.sid.clone()))
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError> {
        self.call(format!("account_name {}", rid(sid)))?;
        let found = self
            .state
            .accounts
            .iter()
            .find(|(_, account)| account.sid == *sid);
        Ok(found.map(|(name, _)| name.clone()))
    }

    fn group(&mut self) -> Result<Option<Sid>, MachineError> {
        self.call("group")?;
        Ok(self.state.group.clone())
    }

    fn create_group(&mut self) -> Result<Sid, MachineError> {
        self.call("create_group")?;
        let group = self.new_sid();
        self.state.group = Some(group.clone());
        Ok(group)
    }

    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError> {
        self.call(format!("group_members {}", rid(group)))?;
        Ok(self
            .state
            .accounts
            .values()
            .filter(|a| a.groups.contains(group))
            .count())
    }

    fn in_group(&mut self, member: &Sid, group: &Sid) -> Result<bool, MachineError> {
        self.call(format!("in_group {} {}", rid(member), rid(group)))?;
        Ok(self
            .state
            .accounts
            .values()
            .any(|a| a.sid == *member && a.groups.contains(group)))
    }

    fn group_facts(&mut self, group: &Sid) -> Result<GroupFacts, MachineError> {
        self.call(format!("group_facts {}", rid(group)))?;
        Ok(self.facts_of(group))
    }

    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.call(format!("delete_group {}", rid(group)))?;
        if self.state.group.as_ref() == Some(group) {
            self.state.group = None;
        }
        for account in self.state.accounts.values_mut() {
            account.groups.remove(group);
        }
        Ok(())
    }

    fn new_password(&mut self) -> Password {
        self.passwords += 1;
        Password::new(format!("password {}", self.passwords))
    }

    fn create_user(&mut self, name: &str, password: &Password) -> Result<Sid, MachineError> {
        self.call(format!("create_user {name}"))?;
        if self.state.accounts.contains_key(name) {
            return Err(MachineError::Os {
                call: "NetUserAdd",
                code: 2224,
            });
        }
        let sid = if self.owner_sid_for.as_deref() == Some(name) {
            Sid::parse(OWNER).unwrap()
        } else {
            self.new_sid()
        };
        let account = Account {
            sid: sid.clone(),
            password: password.to_string(),
            groups: BTreeSet::new(),
            hidden: false,
        };
        self.state.accounts.insert(name.to_owned(), account);
        Ok(sid)
    }

    fn set_password(&mut self, name: &str, password: &Password) -> Result<(), MachineError> {
        self.call(format!("set_password {name}"))?;
        self.account(name)?.password = password.to_string();
        Ok(())
    }

    fn add_to_group(&mut self, name: &str, group: &Sid) -> Result<(), MachineError> {
        self.call(format!("add_to_group {name}"))?;
        if self.state.group.as_ref() != Some(group) {
            return Err(MachineError::Os {
                call: "NetLocalGroupAddMembers",
                code: 2220,
            });
        }
        self.account(name)?.groups.insert(group.clone());
        Ok(())
    }

    fn hide(&mut self, name: &str) -> Result<(), MachineError> {
        self.call(format!("hide {name}"))?;
        self.account(name)?.hidden = true;
        Ok(())
    }

    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        self.call(format!("delete_user {name}"))?;
        self.account(name)?;
        self.state.accounts.remove(name);
        Ok(())
    }

    fn log_on_once(&mut self, name: &str, password: &Password) -> Result<ProfileDir, MachineError> {
        self.call(format!("log_on_once {name}"))?;
        let account = self.account(name)?;
        if account.password != password.as_str() {
            return Err(MachineError::Os {
                call: "LogonUserW",
                code: 1326,
            });
        }
        let sid = account.sid.clone();
        if self.temporary.contains(name) {
            let path = self.dir.dir("Users/TEMP/AppData/LocalLow");
            let path = path.parent().unwrap().parent().unwrap().to_owned();
            return Ok(ProfileDir {
                path,
                temporary: true,
            });
        }
        let listed = self.state.profiles.get(&sid).cloned();
        let path = self
            .faults
            .logon_path(&self.dir.path().join("Users"), name, listed);
        let folder = path.file_name().unwrap().to_string_lossy().into_owned();
        if self.no_local_low.contains(name) {
            self.dir.dir(&format!("Users/{folder}/AppData"));
            self.state.profiles.insert(sid, path.clone());
            return Ok(ProfileDir {
                path,
                temporary: false,
            });
        }
        let local_low = self.dir.dir(&format!("Users/{folder}/AppData/LocalLow"));
        if self.self_owned_local_low.contains(name) {
            self.owners.insert(local_low, sid.clone());
        }
        self.state.profiles.insert(sid, path.clone());
        Ok(ProfileDir {
            path,
            temporary: false,
        })
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
        self.call(format!("profile_of {}", rid(sid)))?;
        Ok(self.state.profiles.get(sid).map(|path| ProfileDir {
            path: path.clone(),
            temporary: false,
        }))
    }

    fn delete_profile(&mut self, sid: &Sid) -> Result<(), MachineError> {
        self.call(format!("delete_profile {}", rid(sid)))?;
        let name = self
            .state
            .accounts
            .iter()
            .find(|(_, a)| a.sid == *sid)
            .map(|(n, _)| n);
        if let Some(path) = self.state.profiles.remove(sid) {
            self.faults.deleted(name, path);
        }
        Ok(())
    }

    fn profiles_dir(&mut self) -> Result<PathBuf, MachineError> {
        self.call("profiles_dir")?;
        Ok(self.dir.path().join("Users"))
    }

    fn folder_names(&mut self, dir: &Path) -> Result<Vec<String>, MachineError> {
        self.call(format!("folder_names {}", folder_name(dir)))?;
        Ok(self.faults.names_in(dir))
    }

    fn owner_of(&mut self, path: &Path) -> Result<Sid, MachineError> {
        self.call(format!("owner_of {}", folder_name(path)))?;
        Ok(self
            .owners
            .get(path)
            .cloned()
            .unwrap_or_else(|| Sid::parse(SYSTEM).unwrap()))
    }

    /// Logged as `exists`, then `owner_of` when it is there: the probe and the read it stands
    /// for on a profile's folder.
    fn owner_in_profile(
        &mut self,
        profile: &Path,
        folder: &Path,
    ) -> Result<Option<Sid>, MachineError> {
        if self.changed_profiles.contains(&folder_name(profile)) {
            self.call(format!("exists {}", folder_name(folder)))?;
            return Err(MachineError::ProfileChanged("AppData".to_owned()));
        }
        if !self.exists(folder)? {
            return Ok(None);
        }
        self.owner_of(folder).map(Some)
    }

    fn exists(&mut self, path: &Path) -> Result<bool, MachineError> {
        self.call(format!("exists {}", folder_name(path)))?;
        Ok(!self.faults.is_gone(path) && std::fs::symlink_metadata(path).is_ok())
    }

    fn give_to_admins(&mut self, profile: &Path, folder: &Path) -> Result<(), MachineError> {
        self.call(format!("give_to_admins {}", folder_name(folder)))?;
        if self.changed_profiles.contains(&folder_name(profile)) {
            return Err(MachineError::ProfileChanged("AppData".to_owned()));
        }
        if !self.stuck_owners.contains(&folder_name(folder)) {
            self.owners
                .insert(folder.to_owned(), Sid::parse(ADMINISTRATORS).unwrap());
        }
        Ok(())
    }

    fn own_account(&self) -> Option<&Sid> {
        None
    }

    fn program_data(&mut self) -> Result<PathBuf, MachineError> {
        self.call("program_data")?;
        Ok(self.dir.path().join("ProgramData"))
    }

    fn read_marker(&mut self) -> MarkerState {
        if !self.acting.get() {
            self.outside_owner.push("read_marker".to_owned());
        }
        self.calls.push("read_marker".to_owned());
        self.state
            .marker
            .clone()
            .map_or(MarkerState::Missing, MarkerState::Current)
    }

    fn write_marker(&mut self, marker: &Marker) -> Result<(), MachineError> {
        self.owner_call("write_marker")?;
        self.state.marker = Some(marker.clone());
        Ok(())
    }

    fn sessions_open(&mut self) -> Result<bool, MachineError> {
        self.owner_call("sessions_open")?;
        Ok(self.sessions_open)
    }

    fn registered_matters(
        &mut self,
        _trusted: &[Sid],
    ) -> Result<Vec<RegisteredMatter>, MachineError> {
        self.owner_call("registered_matters")?;
        Ok(self.matters.clone())
    }

    fn resets_due(&mut self) -> Result<Vec<Sid>, MachineError> {
        self.owner_call("resets_due")?;
        Ok(self.needs_reset.iter().cloned().collect())
    }

    fn reset_done(&mut self, users: &[Sid]) -> Result<(), MachineError> {
        self.owner_call(&format!("reset_done {}", rids(users)))?;
        for user in users {
            self.needs_reset.remove(user);
        }
        Ok(())
    }

    fn stop_resets(&mut self) -> Result<Vec<Sid>, MachineError> {
        assert!(!self.acting.get(), "the record is read as admin");
        self.call("stop_resets")?;
        Ok(self.stop_resets.iter().cloned().collect())
    }

    fn stop_resets_done(&mut self, users: &[Sid]) -> Result<(), MachineError> {
        assert!(!self.acting.get(), "the record is written as admin");
        self.call(format!("stop_resets_done {}", rids(users)))?;
        for user in users {
            self.stop_resets.remove(user);
        }
        Ok(())
    }

    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, MachineError> {
        Ok(self.state.copy_owner.clone())
    }

    fn register_refresh_task(&mut self, owner: &Sid) -> Result<(), MachineError> {
        assert!(!self.acting.get(), "the task is registered as admin");
        self.call(format!("register_refresh_task {}", rid(owner)))?;
        self.state.refresh_task = Some(owner.clone());
        Ok(())
    }

    fn register_stop_task(&mut self, owner: &Sid) -> Result<(), MachineError> {
        assert!(!self.acting.get(), "the task is registered as admin");
        self.call(format!("register_stop_task {}", rid(owner)))?;
        self.state.stop_task = Some(owner.clone());
        Ok(())
    }

    fn ensure_set_up_task(
        &mut self,
        owner: &Sid,
        group: Option<&Sid>,
    ) -> Result<Option<crate::windows::setup::Ensured>, MachineError> {
        assert!(!self.acting.get(), "the task is registered as admin");
        self.heal_set_up_task(owner, group)
    }

    fn event_log(&mut self, line: &str) {
        self.calls.push(format!("event_log {line}"));
    }

    fn deny_set_up_to(&mut self, group: &Sid) -> Result<bool, MachineError> {
        assert!(!self.acting.get(), "the task's descriptor is set as admin");
        // The group is checked through `set_up_denied`: its SID isn't known before the run.
        self.call("deny_set_up_to")?;
        self.state.set_up_denied = Some(group.clone());
        Ok(true)
    }

    fn held_users(
        &mut self,
        _pool: &[Sid],
        _trusted: &[Sid],
        resets: &[Sid],
    ) -> Result<Vec<Sid>, MachineError> {
        self.owner_call("held_users")?;
        self.held_for.push(resets.to_vec());
        Ok(self.held.iter().cloned().collect())
    }

    fn ledger(&mut self, trusted: &[Sid]) -> io::Result<Ledger> {
        self.owner_call("ledger").map_err(io::Error::other)?;
        let reserved = Reserved::new([Sid::parse(OWNER).unwrap()]);
        Ledger::open(ledger_path(&self.data_dir()), trusted, &reserved)
    }

    fn surroundings(&mut self) -> &mut dyn Surroundings {
        &mut self.around
    }

    fn journal_volumes(&mut self, _roots: &[&Path]) -> Result<Vec<PathBuf>, MachineError> {
        self.call("journal_volumes")?;
        Ok(self.journals.keys().cloned().collect())
    }

    fn journal_size(&mut self, volume: &Path) -> Result<Option<u64>, MachineError> {
        self.call(format!("journal_size {}", volume.display()))?;
        Ok(self.journals.get(volume).copied().flatten())
    }

    fn grow_journal(&mut self, volume: &Path, max: u64, _delta: u64) -> Result<(), MachineError> {
        self.call(format!("grow_journal {}", volume.display()))?;
        self.journals.insert(volume.to_owned(), Some(max));
        Ok(())
    }

    fn network(&mut self, setup: &NetworkSetup) -> Result<NetworkReport, MachineError> {
        self.call(format!("network {}", rids(setup.pool())))?;
        Ok(self.around.network(setup))
    }

    fn protect_for_owner(
        &mut self,
        secrets: &[(Sid, Password)],
        pool: &[Sid],
    ) -> Result<(), MachineError> {
        self.protect(secrets, pool)
    }

    fn now(&mut self) -> u64 {
        1_800_000_000
    }

    fn owner(&self) -> &Sid {
        &self.owner
    }

    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError> {
        if self.refuse_owner {
            return Err(MachineError::Other(
                "injected: impersonating the owner".to_owned(),
            ));
        }
        if self.acting.get() {
            return Ok(f(self));
        }
        self.acting.set(true);
        let done = f(self);
        self.acting.set(false);
        Ok(done)
    }
}
