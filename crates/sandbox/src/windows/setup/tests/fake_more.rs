//! More of the set-up's fakes (split from `fake.rs`): the surroundings the fence's walks get,
//! how the call lines name a folder, the profile faults the reset tests inject, the
//! [`FakeMachine`] helpers outside its trait impl (the call recording, the accounts, and
//! [`FakeMachine::new`] (`new_machine`)).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::windows::acl_plan::Sid;
use crate::windows::marker::Marker;
use crate::windows::pool::MachineRecord;
use crate::windows::setup::record_values::CopyOwner;

use std::cell::Cell;
use std::rc::Rc;

use super::fake::{FakeMachine, OWNER, OWNER_NAME, VOLUME, sid};
use crate::test_support::TempDir;
use crate::windows::setup::{LogonRight, MachineError, ServiceStart};

use crate::windows::FolderSupport;
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::wfp::{FirewallOutcome, Namespace, NetworkReport, NetworkSetup};

/// A folder as the call lines name it: a profile's `LocalLow` by its account
/// (`ClauseoSb01`), anything else by its last part.
pub(super) fn folder_name(path: &Path) -> String {
    let account = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name);
    match (path.file_name(), account) {
        (Some(last), Some(account)) if last == "LocalLow" => account.to_string_lossy().into_owned(),
        _ => path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    }
}

/// The surroundings: every folder `Ok`, the real listing, no set-aside, no credentials. It also
/// keeps the network steps' inputs (S15 PR7), so `fake.rs` stays short.
#[derive(Debug, Default)]
pub(super) struct Around {
    /// Each network step's input, in order.
    pub(super) networks: Vec<NetworkSetup>,
    /// What the firewall answers; [`FirewallOutcome::Applied`] when `None`.
    pub(super) firewall: Option<FirewallOutcome>,
}

impl Around {
    /// A network step: recorded, every block filter built, the firewall as told.
    pub(super) fn network(&mut self, setup: &NetworkSetup) -> NetworkReport {
        self.networks.push(setup.clone());
        NetworkReport {
            filters: setup.filters(&Namespace::test()).len(),
            foreign_removed: 0,
            covered: setup.pool().to_vec(),
            firewall: self.firewall.clone().unwrap_or(FirewallOutcome::Applied),
        }
    }
}

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

/// The profile faults the reset tests inject (`resets.rs`), and the folders deleted.
#[derive(Debug, Default)]
pub(super) struct ProfileFaults {
    /// Accounts whose profile folder survives `delete_profile`.
    pub(super) stuck: BTreeSet<String>,
    /// Accounts whose logon makes `<name>.HOST` instead of `<name>`.
    pub(super) moved: BTreeSet<String>,
    /// Accounts whose logon gives their folder's path in lower case.
    pub(super) lower: BTreeSet<String>,
    /// The profile folders `delete_profile` deleted, until a logon makes them again.
    gone: BTreeSet<PathBuf>,
}

impl ProfileFaults {
    /// Where a logon of `name` finds or makes its profile under `users`, as Windows does: the
    /// folder `ProfileList` lists (`listed`); else `<name>`, or `<name>.PC` while a folder by
    /// that name is there; `<name>.HOST` when the test moves it, `<lower-case name>` when the
    /// test changes its case. The folder made is no longer gone.
    pub(super) fn logon_path(
        &mut self,
        users: &Path,
        name: &str,
        listed: Option<PathBuf>,
    ) -> PathBuf {
        let path = listed.unwrap_or_else(|| {
            let own = users.join(name);
            if self.moved.contains(name) {
                users.join(format!("{name}.HOST"))
            } else if self.lower.contains(name) {
                users.join(name.to_ascii_lowercase())
            } else if !self.is_gone(&own) && own.exists() {
                users.join(format!("{name}.PC"))
            } else {
                own
            }
        });
        self.gone.remove(&path);
        path
    }

    /// The names in `dir` on disk, less the folders deleted.
    pub(super) fn names_in(&self, dir: &Path) -> Vec<String> {
        let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
        entries
            .filter(|entry| !self.is_gone(&entry.path()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// `name`'s profile at `path` was deleted: gone, unless its folder is stuck.
    pub(super) fn deleted(&mut self, name: Option<&String>, path: PathBuf) {
        if !name.is_some_and(|name| self.stuck.contains(name)) {
            self.gone.insert(path);
        }
    }

    /// Whether `path` is in a deleted profile.
    pub(super) fn is_gone(&self, path: &Path) -> bool {
        self.gone.iter().any(|folder| path.starts_with(folder))
    }
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
    /// The record's `AdminCopyOwner`.
    pub(super) copy_owner: Option<CopyOwner>,
    /// The owner the task `\Clauseo\SandboxStop`'s descriptor names, while it is registered.
    pub(super) stop_task: Option<Sid>,
    /// The SIDs local policy denies "log on through Remote Desktop Services".
    pub(super) remote_denied: BTreeSet<Sid>,
}

/// [`FakeMachine::new`]: a machine with only the owner's account, `DeviceSync`, an empty data
/// folder and a 32 MiB journal on [`VOLUME`].
pub(super) fn new_machine(test: &str) -> FakeMachine {
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
            copy_owner: None,
            stop_task: None,
            remote_denied: BTreeSet::new(),
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
        matters: Vec::new(),
        owners: BTreeMap::new(),
        self_owned_local_low: BTreeSet::new(),
        stuck_owners: BTreeSet::new(),
        changed_profiles: BTreeSet::new(),
        no_local_low: BTreeSet::new(),
        needs_reset: BTreeSet::new(),
        stop_resets: BTreeSet::new(),
        held: BTreeSet::new(),
        held_for: Vec::new(),
        faults: ProfileFaults::default(),
        next_rid: 1000,
        passwords: 0,
        around: Around::default(),
        launch: None,
    }
}

impl FakeMachine {
    /// A machine with only the owner's account, `DeviceSync`, an empty data folder and a 32 MiB
    /// journal on [`VOLUME`].
    pub(super) fn new(test: &str) -> FakeMachine {
        super::fake_more::new_machine(test)
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
    pub(super) fn call(&mut self, line: impl Into<String>) -> Result<(), MachineError> {
        let line = line.into();
        self.calls.push(line.clone());
        assert!(self.panic_on.as_ref() != Some(&line), "a crash in {line}");
        match self.fails.iter().position(|(text, _)| *text == line) {
            Some(at) => Err(self.fails.remove(at).1),
            None => Ok(()),
        }
    }

    /// Records `line` as a call on the owner's files, and as made outside the owner when it was.
    pub(super) fn owner_call(&mut self, line: &str) -> Result<(), MachineError> {
        if !self.acting.get() {
            self.outside_owner.push(line.to_owned());
        }
        self.call(line)
    }

    pub(super) fn new_sid(&mut self) -> Sid {
        self.next_rid += 1;
        sid(self.next_rid - 1)
    }

    pub(super) fn account(&mut self, name: &str) -> Result<&mut Account, MachineError> {
        self.state.accounts.get_mut(name).ok_or(MachineError::Os {
            call: "NetUserGetInfo",
            code: 2221,
        })
    }
}
