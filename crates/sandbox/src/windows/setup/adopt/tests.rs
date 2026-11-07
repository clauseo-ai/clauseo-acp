//! `--adopt` through a fake machine, on every OS: the refusals change nothing, a second user
//! changes nothing, and the steps run in their order.

use std::path::{Path, PathBuf};

use super::*;
use crate::windows::admin_copy::fake::{descriptor, locked_dacl};
use crate::windows::setup::record_values::{AdminValues, CopyOwner};

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
    found: Vec<(PathBuf, Found)>,
    security: Vec<u8>,
    changes: Vec<String>,
    /// The `AdminCopySha256` the record holds, as `write_values` leaves it.
    recorded: Option<String>,
    /// The refresh task's registration fails.
    task_fails: bool,
    set_up_exit: SetupExit,
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
            task_fails: false,
            set_up_exit: SetupExit::Done,
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
    fn program_files(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(r"C:\Program Files"))
    }

    fn running_image(&mut self) -> Result<PathBuf, String> {
        Ok(self.image.clone())
    }

    fn token(&mut self) -> Result<(bool, bool), String> {
        Ok(self.token)
    }

    fn installing_user(&mut self, claimed: &Sid) -> Result<Sid, String> {
        match &self.outer_user {
            Some(user) if user == claimed => Ok(user.clone()),
            Some(_) => Err("the outer installer's user isn't the one it named".to_owned()),
            None => Err("the outer installer can't be resolved".to_owned()),
        }
    }

    fn records(&mut self) -> Result<Records, String> {
        Ok(Records {
            pool_owner: self.owner.clone(),
            copy_owner: self.copy_owner.clone(),
            version: self.version.clone(),
        })
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, String> {
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

    fn security(&mut self, _path: &Path) -> Result<Vec<u8>, String> {
        Ok(self.security.clone())
    }

    fn lock_down(&mut self, dir: &Path, exe: &Path) -> Result<(), String> {
        assert!(same_path(exe, Path::new(ADMIN_EXE)), "{}", exe.display());
        self.changes.push(format!("lock {}", dir.display()));
        Ok(())
    }

    fn sha256(&mut self, exe: &Path) -> Result<String, String> {
        assert!(same_path(exe, Path::new(ADMIN_EXE)), "{}", exe.display());
        Ok(HASH.to_owned())
    }

    fn write_values(&mut self, values: &AdminValues) -> Result<(), String> {
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

    fn register_refresh_task(&mut self, owner: &Sid) -> Result<(), String> {
        if self.task_fails {
            return Err("the Task Scheduler refused".to_owned());
        }
        self.changes.push(format!("task {owner}"));
        Ok(())
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        self.changes.push(format!("set-up {}", data_dir.display()));
        self.set_up_exit
    }
}

fn user_with(sid: &str, data: &str) -> Target {
    Target::User {
        owner_sid: Sid::parse(sid).unwrap(),
        data_dir: PathBuf::from(data),
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
fn a_per_user_adopt_locks_registers_the_task_records_then_sets_up_in_that_order() {
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert_eq!(fake.changes.len(), 4, "{:?}", fake.changes);
    assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
    let version = admin_copy::APP_VERSION;
    assert_eq!(fake.changes[1], format!("task {LAWYER}"));
    assert_eq!(
        fake.changes[2],
        format!("record {HASH} {INSTALL} {version} {LAWYER}")
    );
    assert_eq!(fake.changes[3], format!("set-up {DATA}"));
}

#[test]
fn a_refresh_task_that_cant_be_registered_leaves_no_record_so_the_check_fails() {
    let mut fake = Fake::new();
    fake.task_fails = true;
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Failed
    );
    assert_eq!(fake.changes.len(), 1, "{:?}", fake.changes);
    assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
    // The launcher's check over what the record then holds: the copy is missing, so tier B
    // isn't used with no task to refresh the copy.
    let mut copy = admin_copy::fake::Fake::ready();
    copy.recorded = fake.recorded.clone();
    assert_eq!(admin_copy::check(&copy), Err(Blocked::AdminCopyMissing));
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
    // Lock and record only: no refresh task, since a per-machine install doesn't update itself.
    assert_eq!(fake.changes.len(), 2, "{:?}", fake.changes);
    let version = admin_copy::APP_VERSION;
    assert_eq!(
        fake.changes[1],
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
