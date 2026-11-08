//! `--remove`'s seam over the set-up's fake: [`Remover`] wraps a [`FakeMachine`] (whose state a
//! set-up built) and [`RemoveAcl`] wraps its [`FakeAcl`]. Both write each call, in order, as a
//! line into one shared log, so a test sees the machine's calls and the permission removals in
//! one sequence; and the call at a chosen place in that log fails, once.

use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clauseo_protocol::MatterId;

use super::fake::{FakeAcl, FakeMachine, OWNER, rid, rids};
use crate::windows::acl_plan::{Grant, Sid};
use crate::windows::apply::Surroundings;
use crate::windows::ledger::{Ledger, RecordedFile, ledger_path};
use crate::windows::pool::MachineRecord;
use crate::windows::refresh::SystemTask;
use crate::windows::set_aside::forget_bookkeeping;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::setup::remove::Removal;
use crate::windows::setup::{Machine, MachineError, ProfileDir};
use crate::windows::shared_writes::OWNER_RIGHTS_SID;
use crate::windows::sids::Reserved;
use crate::windows::writer::{
    AclWriter, Added, ApplyFailure, FileId, Label, Reach, Strict, Walked,
};
use crate::windows::wsl::WslRemoved;

/// The lines, shared by the machine and the writer.
pub(super) type Log = Rc<RefCell<Vec<String>>>;

/// The log's place whose call fails, shared by the machine and the writer.
pub(super) type FailAt = Rc<Cell<Option<usize>>>;

/// Pushes `line`; an error when it landed at `fail_at`, which is then cleared.
fn push(log: &Log, fail_at: &FailAt, line: String) -> bool {
    let mut lines = log.borrow_mut();
    lines.push(line);
    let failing = fail_at.get() == Some(lines.len() - 1);
    if failing {
        fail_at.set(None);
    }
    failing
}

/// `path` below `base`, with `/` between its parts.
fn rel(base: &Path, path: &Path) -> String {
    let below = path.strip_prefix(base).unwrap_or(path);
    let parts: Vec<String> = below
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// The parent's token, for acting as the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParentToken {
    /// A standard user's: the parent's own, or an elevated parent's lowered copy.
    Standard,
    /// An elevated parent's, which couldn't be lowered: never acted with.
    CantLower,
}

/// What another set-up does to the record while the removal waits for the lock.
#[derive(Debug, Clone)]
pub(super) enum Change {
    /// Deletes it.
    Delete,
    /// Writes this record.
    Write(MachineRecord),
}

/// The removal's machine over the set-up's fake.
pub(super) struct Remover {
    pub(super) m: FakeMachine,
    pub(super) log: Log,
    pub(super) fail_at: FailAt,
    /// The verified parent's token user.
    pub(super) invoker: Option<Sid>,
    /// Another set-up holds the lock past the wait.
    pub(super) lock_held: bool,
    /// The record's data folder fails its check.
    pub(super) data_dir_bad: bool,
    /// The folders an administrator's path check passes: the owner's, with no link.
    pub(super) admin_owned: Vec<PathBuf>,
    /// Whether anything ran acting as the owner.
    pub(super) acted: bool,
    /// The parent's token, as `remove::acting_token` left it.
    pub(super) token: ParentToken,
    /// What another set-up does to the record while the removal waits for the lock.
    pub(super) after_lock: Option<Change>,
    /// The matters whose set-aside bookkeeping was deleted, in order.
    pub(super) forgot: Vec<MatterId>,
    /// How many pool processes each sweep can't stop.
    pub(super) strays_left: usize,
    /// Why the firewall rules' removal fails, when it does (WFP's succeeds).
    pub(super) firewall_failure: Option<&'static str>,
    base: PathBuf,
}

impl Remover {
    /// The removal's machine over `m`, for `invoker`.
    pub(super) fn new(m: FakeMachine, invoker: Option<Sid>) -> Remover {
        let base = m.dir.path().to_owned();
        Remover {
            m,
            log: Log::default(),
            fail_at: FailAt::default(),
            invoker,
            lock_held: false,
            data_dir_bad: false,
            admin_owned: Vec::new(),
            acted: false,
            token: ParentToken::Standard,
            after_lock: None,
            forgot: Vec::new(),
            strays_left: 0,
            firewall_failure: None,
            base,
        }
    }

    /// The writer over `acl`, sharing this machine's log and failure.
    pub(super) fn acl(&self, acl: FakeAcl) -> RemoveAcl {
        RemoveAcl {
            acl,
            log: Rc::clone(&self.log),
            fail_at: Rc::clone(&self.fail_at),
            acting: Rc::clone(&self.m.acting),
            base: self.base.clone(),
            removes: Vec::new(),
        }
    }

    /// The lines so far.
    pub(super) fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// Logs `line`, failing it when it lands at the chosen place.
    fn call(&mut self, line: impl Into<String>) -> Result<(), MachineError> {
        let line = line.into();
        if push(&self.log, &self.fail_at, line.clone()) {
            return Err(MachineError::Other(format!("injected: {line}")));
        }
        Ok(())
    }

    /// The record's data folder, checked as the real machine checks it, in the fake's terms.
    fn check_data_dir(&self, data_dir: &Path) -> Result<Option<PathBuf>, MachineError> {
        if self.data_dir_bad || data_dir != self.m.data_dir() {
            return Err(MachineError::Other(
                "the record's data folder isn't its owner's own".to_owned(),
            ));
        }
        let path = ledger_path(data_dir);
        Ok(path.exists().then_some(path))
    }
}

impl Removal for Remover {
    fn lock_within(&mut self, wait_ms: u32) -> Result<(), MachineError> {
        self.call(format!("lock {wait_ms}"))?;
        if self.lock_held {
            return Err(MachineError::LockHeld);
        }
        if wait_ms > 0
            && let Some(change) = self.after_lock.take()
        {
            self.m.state.record = match change {
                Change::Delete => None,
                Change::Write(record) => Some(record),
            };
        }
        Ok(())
    }

    fn unlock(&mut self) {
        self.log.borrow_mut().push("unlock".to_owned());
    }

    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        self.call("read_record")?;
        Machine::read_record(&mut self.m)
    }

    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError> {
        let flag = if record.removing { " removing" } else { "" };
        self.call(format!("write_record{flag}"))?;
        Machine::write_record(&mut self.m, record)
    }

    fn delete_record(&mut self) -> Result<(), MachineError> {
        self.call("delete_record")?;
        Machine::delete_record(&mut self.m)
    }

    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, MachineError> {
        Machine::copy_owner(&mut self.m)
    }

    fn unregister_task(&mut self, task: SystemTask) -> Result<(), MachineError> {
        self.call(format!("unregister_task {}", task.name()))?;
        match task {
            SystemTask::Stop => self.m.state.stop_task = None,
            SystemTask::WslDeny => self.m.state.wsl_task = None,
            SystemTask::WslUpgrade => self.m.state.wsl_upgrade_task = None,
            SystemTask::Refresh => panic!("the removal never deletes the refresh alone"),
        }
        Ok(())
    }

    fn invoker(&self) -> Option<&Sid> {
        self.invoker.as_ref()
    }

    fn can_act_as_owner(&self) -> bool {
        self.invoker.is_some() && self.token == ParentToken::Standard
    }

    fn stop_strays(&mut self, pool: &[Sid], owner: &Sid) -> Result<usize, MachineError> {
        assert_eq!(
            owner.as_str(),
            OWNER,
            "strays are checked against the record's owner"
        );
        self.call(format!("stop_strays {}", rids(pool)))?;
        Ok(self.strays_left)
    }

    fn remove_network(&mut self, owner: &Sid) -> Result<Option<String>, MachineError> {
        assert_eq!(
            owner.as_str(),
            OWNER,
            "the grant taken back is the record's owner's"
        );
        self.call("remove_network".to_owned())?;
        Ok(self.firewall_failure.map(str::to_owned))
    }

    fn remove_wsl_deny(&mut self, group: &Sid) -> Result<WslRemoved, MachineError> {
        self.call(format!("remove_wsl_deny {}", rid(group)))?;
        Ok(WslRemoved::default())
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError> {
        self.call(format!("account_name {}", rid(sid)))?;
        Machine::account_name(&mut self.m, sid)
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
        self.call(format!("profile_of {}", rid(sid)))?;
        Machine::profile_of(&mut self.m, sid)
    }

    fn in_profile(&mut self, profile: &Path, folder: &Path) -> Result<bool, MachineError> {
        Machine::owner_in_profile(&mut self.m, profile, folder).map(|owner| owner.is_some())
    }

    fn delete_profile(&mut self, sid: &Sid, owner: &Sid) -> Result<(), MachineError> {
        assert_ne!(sid, owner, "the owner's profile is never deleted");
        self.call(format!("delete_profile {}", rid(sid)))?;
        Machine::delete_profile(&mut self.m, sid)
    }

    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        self.call(format!("delete_user {name}"))?;
        Machine::delete_user(&mut self.m, name)
    }

    fn program_data(&mut self) -> Result<PathBuf, MachineError> {
        self.call("program_data")?;
        Machine::program_data(&mut self.m)
    }

    fn group(&mut self) -> Result<Option<Sid>, MachineError> {
        self.call("group")?;
        Machine::group(&mut self.m)
    }

    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError> {
        self.call(format!("group_members {}", rid(group)))?;
        Machine::group_members(&mut self.m, group)
    }

    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.call(format!("delete_group {}", rid(group)))?;
        Machine::delete_group(&mut self.m, group)
    }

    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError> {
        if !self.can_act_as_owner() {
            return Err(MachineError::Other(
                "no standard token to act as".to_owned(),
            ));
        }
        if self.m.acting.get() {
            return Ok(f(self));
        }
        self.acted = true;
        self.m.acting.set(true);
        let done = f(self);
        self.m.acting.set(false);
        Ok(done)
    }

    fn owners_ledger(
        &mut self,
        data_dir: &Path,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError> {
        assert!(
            self.m.acting.get(),
            "the owner's ledger is read as the owner"
        );
        self.call("owners_ledger")?;
        if self.check_data_dir(data_dir)?.is_none() {
            return Ok(None);
        }
        Machine::ledger(&mut self.m, trusted)
            .map(Some)
            .map_err(|error| MachineError::Other(error.to_string()))
    }

    fn surroundings(&mut self) -> &mut dyn Surroundings {
        Machine::surroundings(&mut self.m)
    }

    fn forget_set_aside(&mut self, matter: &MatterId) -> Result<(), MachineError> {
        assert!(
            self.m.acting.get(),
            "a set-aside's bookkeeping is deleted only as the owner"
        );
        self.forgot.push(*matter);
        forget_bookkeeping(&self.m.data_dir(), matter)
            .map(drop)
            .map_err(|failure| MachineError::Other(failure.to_string()))
    }

    fn admins_ledger(
        &mut self,
        data_dir: &Path,
        owner: &Sid,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError> {
        assert!(!self.m.acting.get(), "an administrator acts as itself");
        assert_eq!(owner.as_str(), OWNER);
        self.call("admins_ledger")?;
        let Some(path) = self.check_data_dir(data_dir)? else {
            return Ok(None);
        };
        let reserved = Reserved::new([Sid::parse(OWNER).unwrap()]);
        Ledger::open(path, trusted, &reserved)
            .map(Some)
            .map_err(|error| MachineError::Other(error.to_string()))
    }

    fn admin_target(&mut self, path: &Path, owner: &Sid) -> Result<(), MachineError> {
        assert!(!self.m.acting.get(), "an administrator acts as itself");
        assert_eq!(owner.as_str(), OWNER);
        self.call(format!("admin_target {}", rel(&self.base, path)))?;
        if self.admin_owned.iter().any(|owned| owned == path) {
            Ok(())
        } else {
            Err(MachineError::Other(
                "a matter's folder isn't its owner's own".to_owned(),
            ))
        }
    }
}

/// The removal's permission writes over the set-up's fake writer, logged.
pub(super) struct RemoveAcl {
    pub(super) acl: FakeAcl,
    log: Log,
    fail_at: FailAt,
    acting: Rc<Cell<bool>>,
    base: PathBuf,
    /// Each removal, and whether the thread acted as the owner then.
    pub(super) removes: Vec<(String, bool)>,
}

impl RemoveAcl {
    /// Logs `what` on `path` for `sid`, failing it when it lands at the chosen place.
    fn call(&mut self, what: &str, path: &Path, sid: Option<&Sid>) -> Result<(), ApplyFailure> {
        let mut line = format!("{what} {}", rel(&self.base, path));
        if let Some(sid) = sid {
            let subject = if sid.as_str() == OWNER_RIGHTS_SID {
                "owner-rights".to_owned()
            } else {
                rid(sid)
            };
            line = format!("{line} {subject}");
        }
        if push(&self.log, &self.fail_at, line) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        Ok(())
    }
}

impl AclWriter for RemoveAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        self.call("add", grant.path(), Some(grant.sid()))?;
        self.acl.add(grant)
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        self.call("remove", grant.path(), Some(grant.sid()))?;
        let line = format!("{} {}", rel(&self.base, grant.path()), rid(grant.sid()));
        self.removes.push((line, self.acting.get()));
        self.acl.remove(grant)
    }

    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        self.acl.present(grant)
    }

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.call("walk", root, None)?;
        self.acl.propagate(root, reach, strict)
    }

    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool> {
        self.acl.same_file(a, b)
    }

    fn file_id(&mut self, path: &Path) -> io::Result<Option<FileId>> {
        self.acl.file_id(path)
    }

    fn remove_recorded(
        &mut self,
        file: &RecordedFile,
        grant: &Grant,
        prior: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        self.call("remove_recorded", grant.path(), Some(grant.sid()))?;
        self.acl.remove_recorded(file, grant, prior)
    }

    fn present_recorded(&mut self, file: &RecordedFile, grant: &Grant) -> io::Result<bool> {
        self.acl.present_recorded(file, grant)
    }

    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure> {
        self.call("add", grant.path(), Some(grant.sid()))?;
        self.acl.add_expecting(grant, expected)
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        None
    }

    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.call("reclaim", root, None)?;
        self.acl.reclaim(root, pool)
    }

    fn hand_back_tree(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.acl.hand_back_tree(root, pool)
    }

    fn hand_back(&mut self, path: &Path, pool: &[Sid]) -> Result<bool, ApplyFailure> {
        self.acl.hand_back(path, pool)
    }

    fn label(&mut self, path: &Path) -> io::Result<Option<Label>> {
        self.acl.label(path)
    }

    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure> {
        self.call("restore_label", grant.path(), None)?;
        self.acl.restore_label(grant, label)
    }
}
