//! `--adopt` through a fake machine, on every OS: the refusals change nothing, a second user
//! changes nothing, and the steps run in their order.

use std::path::{Path, PathBuf};

use super::*;
use crate::windows::admin_copy::fake::{descriptor, locked_dacl};
use crate::windows::setup::explain::Code;
use crate::windows::setup::record_values::{AdminValues, CopyOwner};

mod place_only;
mod reports;
mod set_up_task;

const LAWYER: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const BESIDE_APP: &str = r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe";
const INSTALL: &str = r"C:\Users\lawyer\AppData\Local\Clauseo";
const DATA: &str = r"C:\Users\lawyer\AppData\Local\ai.clauseo.desktop";
const MACHINE: &str = r"D:\Apps\Clauseo";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The fake: what each read gives, and every change asked for, in order.
struct Fake {
    image: PathBuf,
    token: (bool, bool),
    /// The unelevated installer's token user; `None` when it can't be resolved.
    outer_user: Option<Sid>,
    owner: Option<Sid>,
    copy_owner: Option<CopyOwner>,
    version: Option<String>,
    /// The record's `GroupSid`.
    group: Option<Sid>,
    found: Vec<(PathBuf, Found)>,
    security: Vec<u8>,
    changes: Vec<String>,
    /// The `AdminCopySha256` the record holds, as `write_values` leaves it.
    recorded: Option<String>,
    /// Another holds the set-up lock.
    lock_busy: bool,
    /// `--adopt` holds the set-up lock now.
    held: bool,
    /// This task's registration fails.
    failing_task: Option<SystemTask>,
    set_up_exit: SetupExit,
    /// This step's call fails, with Win32 error 5.
    fail: Option<AdoptStep>,
    /// Every failure reported, in order.
    reports: Vec<AdoptFailure>,
}

impl Fake {
    fn new() -> Fake {
        let folder = |path: &str| (PathBuf::from(path), Found::Folder);
        Fake {
            image: PathBuf::from(ADMIN_EXE),
            token: (true, false),
            outer_user: Some(lawyer()),
            owner: None,
            copy_owner: None,
            version: None,
            group: None,
            found: vec![
                folder(r"C:\Program Files"),
                folder(r"C:\Program Files\Clauseo"),
                folder(r"C:\Program Files\Clauseo\Sandbox"),
                (PathBuf::from(ADMIN_EXE), Found::File),
                folder(INSTALL),
                folder(MACHINE),
                folder(r"C:\Users\other\AppData\Local\Clauseo"),
                folder(r"C:\Users\lawyer"),
            ],
            security: locked_dacl(true),
            changes: Vec::new(),
            recorded: None,
            lock_busy: false,
            held: false,
            failing_task: None,
            set_up_exit: SetupExit::Done,
            fail: None,
            reports: Vec::new(),
        }
    }

    /// `Err` with Win32 error 5 when `step` is the one set to fail.
    fn failing(&self, step: AdoptStep) -> Result<(), Cause> {
        match self.fail {
            Some(failing) if failing == step => Err(Cause::with("denied", Code::Win32(5))),
            _ => Ok(()),
        }
    }

    fn set(&mut self, path: &str, found: Found) {
        self.found.retain(|(at, _)| !same_path(at, Path::new(path)));
        self.found.push((PathBuf::from(path), found));
    }
}

fn lawyer() -> Sid {
    Sid::parse(LAWYER).unwrap()
}

impl Adopter for Fake {
    fn program_files(&mut self) -> Result<PathBuf, Cause> {
        self.failing(AdoptStep::ProgramFiles)?;
        Ok(PathBuf::from(r"C:\Program Files"))
    }

    fn running_image(&mut self) -> Result<PathBuf, Cause> {
        self.failing(AdoptStep::RunningImage)?;
        Ok(self.image.clone())
    }

    fn token(&mut self) -> Result<(bool, bool), Cause> {
        self.failing(AdoptStep::Token)?;
        Ok(self.token)
    }

    fn installing_user(&mut self, claimed: &Sid) -> Result<Sid, Cause> {
        match &self.outer_user {
            Some(user) if user == claimed => Ok(user.clone()),
            Some(_) => Err("the outer installer's user isn't the one it named".into()),
            None => Err("the outer installer can't be resolved".into()),
        }
    }

    fn records(&mut self) -> Result<Records, Cause> {
        self.failing(AdoptStep::Record)?;
        Ok(Records {
            pool_owner: self.owner.clone(),
            copy_owner: self.copy_owner.clone(),
            version: self.version.clone(),
            group: self.group.clone(),
        })
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, Cause> {
        self.failing(AdoptStep::Profile)?;
        let profile = match sid.as_str() {
            LAWYER => r"C:\Users\lawyer",
            OTHER => r"C:\Users\other",
            _ => return Ok(None),
        };
        Ok(Some(PathBuf::from(profile)))
    }

    fn found(&mut self, path: &Path) -> Found {
        self.found
            .iter()
            .find(|(at, _)| same_path(at, path))
            .map_or(Found::Missing, |(_, found)| *found)
    }

    fn security(&mut self, _path: &Path) -> Result<Vec<u8>, Cause> {
        Ok(self.security.clone())
    }

    fn lock(&mut self) -> Result<bool, Cause> {
        assert!(!self.held, "taken once");
        self.failing(AdoptStep::Lock)?;
        self.held = !self.lock_busy;
        Ok(self.held)
    }

    fn unlock(&mut self) {
        self.held = false;
    }

    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), Cause> {
        assert!(
            self.held,
            "the admin folder changes only under the set-up lock"
        );
        assert!(same_path(exe, Path::new(ADMIN_EXE)), "{}", exe.display());
        self.failing(AdoptStep::LockDown)?;
        self.changes.push(format!("lock {}", dir.display()));
        Ok(())
    }

    fn sha256(&mut self, exe: &Path) -> Result<String, Cause> {
        assert!(self.held, "hashed under the set-up lock");
        assert!(same_path(exe, Path::new(ADMIN_EXE)), "{}", exe.display());
        self.failing(AdoptStep::Hash)?;
        Ok(HASH.to_owned())
    }

    fn write_values(&mut self, values: &AdminValues) -> Result<(), Cause> {
        assert!(self.held, "written under the lock the hash was read under");
        self.failing(AdoptStep::Values)?;
        self.changes.push(format!(
            "record {} {} {} {}",
            values.sha256,
            values.install_dir.display(),
            values.version,
            values.owner.text()
        ));
        self.recorded = Some(values.sha256.clone());
        Ok(())
    }

    fn register_task(&mut self, task: SystemTask, starters: &Starters) -> Result<(), Cause> {
        assert!(self.held, "registered under the set-up lock");
        if self.failing_task == Some(task) {
            return Err(Cause::with(
                "the Task Scheduler refused",
                Code::Hresult(0x8004_1318),
            ));
        }
        let starters = match starters {
            Starters::Owner(owner) => owner.to_string(),
            Starters::Interactive { denied: None } => "interactive".to_owned(),
            Starters::Interactive {
                denied: Some(group),
            } => format!("interactive but {group}"),
        };
        self.changes
            .push(format!("task {} {starters}", task.name()));
        Ok(())
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        assert!(!self.held, "given back before the set-up takes it");
        self.changes.push(format!("set-up {}", data_dir.display()));
        self.set_up_exit
    }

    fn report(&mut self, failure: &AdoptFailure) {
        self.reports.push(failure.clone());
    }
}

fn user_with(sid: &str, data: &str) -> Target {
    Target::User {
        owner_sid: Sid::parse(sid).unwrap(),
        data_dir: PathBuf::from(data),
        place_only: false,
    }
}

fn user() -> Target {
    user_with(LAWYER, DATA)
}

fn refused(fake: &mut Fake, install: &str, target: &Target, exit: SetupExit, case: &str) {
    assert_eq!(adopt(fake, Path::new(install), target), exit, "{case}");
    assert_eq!(fake.changes, Vec::<String>::new(), "{case}");
}

#[test]
fn a_per_user_adopt_locks_registers_both_tasks_records_then_sets_up_in_that_order() {
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert_eq!(fake.changes.len(), 5, "{:?}", fake.changes);
    assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
    let version = admin_copy::APP_VERSION;
    assert_eq!(fake.changes[1], format!("task SandboxRefresh {LAWYER}"));
    assert_eq!(fake.changes[2], format!("task SandboxStop {LAWYER}"));
    assert_eq!(
        fake.changes[3],
        format!("record {HASH} {INSTALL} {version} {LAWYER}")
    );
    assert_eq!(fake.changes[4], format!("set-up {DATA}"));
}

#[test]
fn a_task_that_cant_be_registered_leaves_no_record_so_the_check_fails() {
    for (failing, before) in [(SystemTask::Refresh, 1), (SystemTask::Stop, 2)] {
        let mut fake = Fake::new();
        fake.failing_task = Some(failing);
        assert_eq!(
            adopt(&mut fake, Path::new(INSTALL), &user()),
            SetupExit::Failed,
            "{failing:?}"
        );
        // The lock, and the refresh task when the stop task is the one refused; no record.
        assert_eq!(fake.changes.len(), before, "{:?}", fake.changes);
        assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
        assert!(fake.recorded.is_none(), "{failing:?}");
        // The launcher's check over what the record then holds: the copy is missing, so tier B
        // isn't used with no task to refresh the copy or stop the pool's processes.
        let mut copy = admin_copy::fake::Fake::ready();
        copy.recorded = fake.recorded.clone();
        assert_eq!(admin_copy::check(&copy), Err(Blocked::AdminCopyMissing));
    }
    let mut copy = admin_copy::fake::Fake::ready();
    // Registered, the same copy passes.
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    copy.recorded = fake.recorded.clone();
    assert!(admin_copy::check(&copy).is_ok());
}

#[test]
fn the_set_ups_exit_is_adopts() {
    let mut fake = Fake::new();
    fake.set_up_exit = SetupExit::LockHeld;
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::LockHeld
    );
}

#[test]
fn a_named_sid_that_isnt_the_outer_installers_or_an_unresolved_one_refuses() {
    let mut fake = Fake::new();
    let other = user_with(OTHER, r"C:\Users\other\AppData\Local\ai.clauseo.desktop");
    let install = r"C:\Users\other\AppData\Local\Clauseo";
    refused(
        &mut fake,
        install,
        &other,
        SetupExit::Failed,
        "mismatched SID",
    );
    let mut fake = Fake::new();
    fake.outer_user = None;
    refused(
        &mut fake,
        INSTALL,
        &user(),
        SetupExit::Failed,
        "no outer installer",
    );
}

#[test]
fn a_second_user_changes_nothing() {
    let mut fake = Fake::new();
    fake.owner = Some(Sid::parse(OTHER).unwrap());
    refused(
        &mut fake,
        INSTALL,
        &user(),
        SetupExit::OtherOwner,
        "another owner",
    );
    // The owner's own re-install goes on.
    let mut fake = Fake::new();
    fake.owner = Some(lawyer());
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
}

#[test]
fn from_any_place_but_the_admin_copy_it_refuses() {
    for running in [
        BESIDE_APP,
        r"C:\Program Files\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        let mut fake = Fake::new();
        fake.image = PathBuf::from(running);
        refused(
            &mut fake,
            INSTALL,
            &user(),
            SetupExit::NotAdminCopy,
            running,
        );
    }
    // A link anywhere on the way, even when the final path reads right.
    for level in [
        r"C:\Program Files\Clauseo",
        r"C:\Program Files\Clauseo\Sandbox",
        ADMIN_EXE,
    ] {
        let mut fake = Fake::new();
        fake.set(level, Found::Link);
        refused(&mut fake, INSTALL, &user(), SetupExit::NotAdminCopy, level);
    }
    // Letter case and a `\\?\` prefix are the same path.
    let mut fake = Fake::new();
    fake.image = PathBuf::from(format!(r"\\?\{}", ADMIN_EXE.to_lowercase()));
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
}

#[test]
fn without_elevation_it_refuses_and_system_is_only_for_per_machine() {
    let mut fake = Fake::new();
    fake.token = (false, false);
    refused(
        &mut fake,
        INSTALL,
        &user(),
        SetupExit::NotElevated,
        "per user",
    );
    refused(
        &mut fake,
        MACHINE,
        &Target::Machine,
        SetupExit::NotElevated,
        "machine",
    );
    fake.token = (false, true);
    refused(
        &mut fake,
        INSTALL,
        &user(),
        SetupExit::NotElevated,
        "SYSTEM per user",
    );
    assert_eq!(
        adopt(&mut fake, Path::new(MACHINE), &Target::Machine),
        SetupExit::Done
    );
}

#[test]
fn folders_outside_the_lawyers_profile_change_nothing() {
    for (install, data) in [
        (r"C:\Users\other\AppData\Local\Clauseo", DATA),
        (r"C:\Users\lawyer", DATA),
        (r"C:\Program Files\Clauseo", DATA),
        (r"C:\Users\lawyer\AppData\Local\Missing", DATA),
        (INSTALL, r"C:\Users\other\AppData\Local\ai.clauseo.desktop"),
        (INSTALL, r"C:\Users\lawyer"),
        (INSTALL, r"D:\data"),
    ] {
        let mut fake = Fake::new();
        let target = user_with(LAWYER, data);
        let case = format!("{install} {data}");
        refused(&mut fake, install, &target, SetupExit::Usage, &case);
    }
    let mut fake = Fake::new();
    fake.set(INSTALL, Found::Link);
    refused(
        &mut fake,
        INSTALL,
        &user(),
        SetupExit::Usage,
        "a linked install folder",
    );
}

#[test]
fn a_per_machine_adopt_takes_any_administrators_only_folder_and_sets_nobody_up() {
    let mut fake = Fake::new();
    // Never asked: no installing user, and a record of anyone's is left as it is.
    fake.owner = Some(Sid::parse(OTHER).unwrap());
    fake.outer_user = None;
    assert_eq!(
        adopt(&mut fake, Path::new(MACHINE), &Target::Machine),
        SetupExit::Done
    );
    // Lock, the set-up task for interactive users, and record: no refresh or stop task, since a
    // per-machine install doesn't update itself and has no owner yet (its set-up registers the
    // stop task).
    assert_eq!(fake.changes.len(), 3, "{:?}", fake.changes);
    assert_eq!(fake.changes[1], "task SandboxSetUp interactive");
    let version = admin_copy::APP_VERSION;
    assert_eq!(
        fake.changes[2],
        format!("record {HASH} {MACHINE} {version} machine")
    );
    // A folder Users can write, a link, or nothing there refuses.
    let mut fake = Fake::new();
    let users_write = (0, 3, 0x2, "S-1-5-32-545");
    fake.security = descriptor("S-1-5-32-544", Some(&[users_write]), true);
    refused(
        &mut fake,
        MACHINE,
        &Target::Machine,
        SetupExit::Usage,
        "Users write",
    );
    let mut fake = Fake::new();
    fake.set(MACHINE, Found::Link);
    refused(
        &mut fake,
        MACHINE,
        &Target::Machine,
        SetupExit::Usage,
        "link",
    );
    let mut fake = Fake::new();
    refused(
        &mut fake,
        r"D:\None",
        &Target::Machine,
        SetupExit::Usage,
        "missing",
    );
}

#[test]
fn one_owner_of_one_kind_and_another_is_another_account() {
    let other = CopyOwner::User(Sid::parse(OTHER).unwrap());
    // Per user: a copy another user's install placed, or a per-machine one.
    for placed in [other.clone(), CopyOwner::Machine] {
        let mut fake = Fake::new();
        fake.copy_owner = Some(placed.clone());
        refused(
            &mut fake,
            INSTALL,
            &user(),
            SetupExit::OtherOwner,
            placed.text(),
        );
    }
    // Per machine: a copy a per-user install placed. A user's pool on the machine is fine.
    let mut fake = Fake::new();
    fake.copy_owner = Some(other);
    refused(
        &mut fake,
        MACHINE,
        &Target::Machine,
        SetupExit::OtherOwner,
        "per-user copy",
    );
    let mut fake = Fake::new();
    fake.copy_owner = Some(CopyOwner::Machine);
    fake.owner = Some(lawyer());
    assert_eq!(
        adopt(&mut fake, Path::new(MACHINE), &Target::Machine),
        SetupExit::Done
    );
    // The lawyer's own copy is replaced.
    let mut fake = Fake::new();
    fake.copy_owner = Some(CopyOwner::User(lawyer()));
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
}

#[test]
fn a_newer_recorded_copy_is_left_as_it_is() {
    for target in [user(), Target::Machine] {
        let install = if target == Target::Machine {
            MACHINE
        } else {
            INSTALL
        };
        let mut fake = Fake::new();
        fake.version = Some("999.0.0".to_owned());
        refused(&mut fake, install, &target, SetupExit::Downgrade, "newer");
        let mut fake = Fake::new();
        fake.version = Some(admin_copy::APP_VERSION.to_owned());
        assert_eq!(
            adopt(&mut fake, Path::new(install), &target),
            SetupExit::Done
        );
    }
}

#[test]
fn the_set_up_lock_spans_the_hash_and_the_record_and_a_held_one_changes_nothing() {
    // The fake asserts the lock is held at lock_down, sha256 and write_values, and given back
    // before set_up.
    for target in [user(), Target::Machine] {
        let install = if target == Target::Machine {
            MACHINE
        } else {
            INSTALL
        };
        let mut fake = Fake::new();
        assert_eq!(
            adopt(&mut fake, Path::new(install), &target),
            SetupExit::Done
        );
        assert!(!fake.held);
        let mut fake = Fake::new();
        fake.lock_busy = true;
        refused(&mut fake, install, &target, SetupExit::LockHeld, "busy");
    }
    // A refusal under the lock gives it back too.
    let mut fake = Fake::new();
    fake.failing_task = Some(SystemTask::Stop);
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Failed
    );
    assert!(!fake.held);
}
