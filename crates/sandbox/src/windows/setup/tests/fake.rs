//! The set-up's seams faked in memory: [`FakeMachine`] records every call as a line of text and
//! fails the calls it is told to; [`FakeAcl`] (`fake_acl.rs`) keeps the entries written.
//! Profiles and `DeviceSync` are real folders in a temp folder, since the shared-write apply
//! checks they exist, and the ledger is a real file there.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::test_support::TempDir;
use crate::windows::FolderSupport;
use crate::windows::acl_plan::Sid;
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::marker::{Marker, MarkerState};
use crate::windows::pool::MachineRecord;
use crate::windows::setup::{
    LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart,
};
use crate::windows::sids::Reserved;

pub(super) use super::fake_acl::FakeAcl;

/// The owner's SID, and its account's name.
pub(super) const OWNER: &str = "S-1-5-21-7-8-9-1001";
pub(super) const OWNER_NAME: &str = "Rohan";
/// Another Windows user.
pub(super) const OTHER_OWNER: &str = "S-1-5-21-7-8-9-1002";
/// The volume the fake lists for the change journal.
pub(super) const VOLUME: &str = "C:";

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

/// One local account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Account {
    pub(super) sid: Sid,
    pub(super) password: String,
    pub(super) groups: BTreeSet<Sid>,
    pub(super) hidden: bool,
}

/// What the machine holds, for comparing before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct State {
    pub(super) accounts: BTreeMap<String, Account>,
    pub(super) group: Option<Sid>,
    pub(super) record: Option<MachineRecord>,
    pub(super) profiles: BTreeMap<Sid, PathBuf>,
    pub(super) marker: Option<Marker>,
    pub(super) secrets: BTreeMap<Sid, String>,
}

/// The surroundings: every folder `Ok`, the real listing, no set-aside, no credentials.
#[derive(Debug, Default)]
pub(super) struct Around;

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Ok(FolderSupport::Ok)
    }

    fn set_aside(&mut self, _matter: clauseo_protocol::MatterId, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
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
    next_rid: u32,
    passwords: u32,
    around: Around,
}

impl FakeMachine {
    /// A machine with only the owner's account, `DeviceSync`, an empty data folder and a 32 MiB
    /// journal on [`VOLUME`].
    pub(super) fn new(test: &str) -> FakeMachine {
        let dir = TempDir::new(test);
        dir.dir("ProgramData/Microsoft/DeviceSync");
        dir.dir("data");
        let owner = Account {
            sid: Sid::parse(OWNER).unwrap(),
            password: "the owner's".to_owned(),
            groups: BTreeSet::new(),
            hidden: false,
        };
        FakeMachine {
            dir,
            calls: Vec::new(),
            fails: Vec::new(),
            panic_on: None,
            state: State {
                accounts: BTreeMap::from([(OWNER_NAME.to_owned(), owner)]),
                group: None,
                record: None,
                profiles: BTreeMap::new(),
                marker: None,
                secrets: BTreeMap::new(),
            },
            written: Vec::new(),
            temporary: BTreeSet::new(),
            seclogon: ServiceStart::Manual,
            logon_right: LogonRight::Allowed,
            lock_held: false,
            sessions_open: false,
            journals: BTreeMap::from([(PathBuf::from(VOLUME), Some(32 << 20))]),
            owner_sid_for: None,
            owner: Sid::parse(OWNER).unwrap(),
            acting: Rc::new(Cell::new(false)),
            outside_owner: Vec::new(),
            refuse_owner: false,
            next_rid: 1000,
            passwords: 0,
            around: Around,
        }
    }

    /// The data folder.
    pub(super) fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// `%ProgramData%\Microsoft\DeviceSync`.
    pub(super) fn device_sync(&self) -> PathBuf {
        // One component at a time: a joined "a/b" keeps its `/` on Windows, so the path would
        // print with mixed separators and not match the cause text.
        self.dir
            .path()
            .join("ProgramData")
            .join("Microsoft")
            .join("DeviceSync")
    }

    /// The profile folder of the account `name`.
    pub(super) fn profile_path(&self, name: &str) -> PathBuf {
        self.dir.path().join("Users").join(name)
    }

    /// Fails the first call `line` with an injected error.
    pub(super) fn fail(&mut self, line: &str) {
        self.fails.push((
            line.to_owned(),
            MachineError::Other(format!("injected: {line}")),
        ));
    }

    /// The calls since `from`.
    pub(super) fn calls_since(&self, from: usize) -> Vec<&str> {
        self.calls[from..].iter().map(String::as_str).collect()
    }

    /// Records `line`, failing it if told to.
    fn call(&mut self, line: impl Into<String>) -> Result<(), MachineError> {
        let line = line.into();
        self.calls.push(line.clone());
        assert!(self.panic_on.as_ref() != Some(&line), "a crash in {line}");
        match self.fails.iter().position(|(text, _)| *text == line) {
            Some(at) => Err(self.fails.remove(at).1),
            None => Ok(()),
        }
    }

    /// Records `line` as a call on the owner's files, and as made outside the owner when it was.
    fn owner_call(&mut self, line: &str) -> Result<(), MachineError> {
        if !self.acting.get() {
            self.outside_owner.push(line.to_owned());
        }
        self.call(line)
    }

    fn new_sid(&mut self) -> Sid {
        self.next_rid += 1;
        sid(self.next_rid - 1)
    }

    fn account(&mut self, name: &str) -> Result<&mut Account, MachineError> {
        self.state.accounts.get_mut(name).ok_or(MachineError::Os {
            call: "NetUserGetInfo",
            code: 2221,
        })
    }
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
        let path = self.profile_path(name);
        self.dir.dir(&format!("Users/{name}/AppData/LocalLow"));
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
        self.state.profiles.remove(sid);
        Ok(())
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

    fn protect_for_owner(
        &mut self,
        secrets: &[(Sid, Password)],
        pool: &[Sid],
    ) -> Result<(), MachineError> {
        let sids: Vec<Sid> = secrets.iter().map(|(sid, _)| sid.clone()).collect();
        self.owner_call(&format!("protect_for_owner {}", rids(&sids)))?;
        let mut file = BTreeMap::new();
        for sid in pool {
            let password = match secrets.iter().find(|(s, _)| s == sid) {
                Some((_, password)) => password.to_string(),
                None => self.state.secrets.get(sid).cloned().ok_or_else(|| {
                    MachineError::Other(format!("secrets.bin has no blob for {sid}"))
                })?,
            };
            file.insert(sid.clone(), password);
        }
        self.state.secrets = file;
        Ok(())
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
