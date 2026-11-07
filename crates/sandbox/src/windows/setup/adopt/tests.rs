//! `--adopt` through a fake machine, on every OS: the refusals change nothing, a second user
//! changes nothing, and the steps run in their order.

use std::path::{Path, PathBuf};

use super::*;

const LAWYER: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const BESIDE_APP: &str = r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe";
const INSTALL: &str = r"C:\Users\lawyer\AppData\Local\Clauseo";
const DATA: &str = r"C:\Users\lawyer\AppData\Local\chat.clauseo";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The fake: what each read gives, and every change asked for, in order.
struct Fake {
    exe: PathBuf,
    token: (bool, bool),
    user: Sid,
    owner: Option<Sid>,
    folders: Vec<PathBuf>,
    changes: Vec<String>,
    set_up_exit: SetupExit,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            exe: PathBuf::from(ADMIN_EXE),
            token: (true, false),
            user: Sid::parse(LAWYER).unwrap(),
            owner: None,
            folders: vec![
                PathBuf::from(INSTALL),
                PathBuf::from(r"C:\Program Files\Clauseo"),
            ],
            changes: Vec::new(),
            set_up_exit: SetupExit::Done,
        }
    }
}

impl Adopter for Fake {
    fn program_files(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(r"C:\Program Files"))
    }

    fn current_exe(&mut self) -> Result<PathBuf, String> {
        Ok(self.exe.clone())
    }

    fn token(&mut self) -> Result<(bool, bool), String> {
        Ok(self.token)
    }

    fn installing_user(&mut self) -> Result<Sid, String> {
        Ok(self.user.clone())
    }

    fn record_owner(&mut self) -> Result<Option<Sid>, String> {
        Ok(self.owner.clone())
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<PathBuf>, String> {
        Ok((sid.as_str() == LAWYER).then(|| PathBuf::from(r"C:\Users\lawyer")))
    }

    fn is_folder(&mut self, path: &Path) -> bool {
        self.folders.iter().any(|folder| same_path(folder, path))
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

    fn write_values(&mut self, sha256: &str, install_dir: &Path) -> Result<(), String> {
        self.changes
            .push(format!("record {sha256} {}", install_dir.display()));
        Ok(())
    }

    fn set_up(&mut self, data_dir: &Path) -> SetupExit {
        self.changes.push(format!("set-up {}", data_dir.display()));
        self.set_up_exit
    }
}

fn user() -> Target {
    Target::User {
        data_dir: PathBuf::from(DATA),
    }
}

#[test]
fn a_per_user_adopt_locks_records_then_sets_up_in_that_order() {
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert_eq!(fake.changes.len(), 3, "{:?}", fake.changes);
    assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
    assert_eq!(fake.changes[1], format!("record {HASH} {INSTALL}"));
    assert_eq!(fake.changes[2], format!("set-up {DATA}"));
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
fn a_second_user_changes_nothing() {
    let mut fake = Fake::new();
    fake.owner = Some(Sid::parse(OTHER).unwrap());
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::OtherOwner
    );
    assert!(fake.changes.is_empty(), "{:?}", fake.changes);
    // The owner's own re-install goes on.
    let mut fake = Fake::new();
    fake.owner = Some(Sid::parse(LAWYER).unwrap());
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
}

#[test]
fn from_any_path_but_the_admin_copy_it_refuses() {
    for running in [
        BESIDE_APP,
        r"C:\Program Files\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        let mut fake = Fake::new();
        fake.exe = PathBuf::from(running);
        assert_eq!(
            adopt(&mut fake, Path::new(INSTALL), &user()),
            SetupExit::NotAdminCopy,
            "{running}"
        );
        assert!(fake.changes.is_empty(), "{running}: {:?}", fake.changes);
    }
    // Letter case and a `\\?\` prefix are the same path.
    let mut fake = Fake::new();
    fake.exe = PathBuf::from(format!(r"\\?\{}", ADMIN_EXE.to_lowercase()));
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
}

#[test]
fn without_elevation_it_refuses_and_system_is_only_for_per_machine() {
    let machine = Path::new(r"C:\Program Files\Clauseo");
    let mut fake = Fake::new();
    fake.token = (false, false);
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::NotElevated
    );
    assert_eq!(
        adopt(&mut fake, machine, &Target::Machine),
        SetupExit::NotElevated
    );
    fake.token = (false, true);
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::NotElevated
    );
    assert!(fake.changes.is_empty(), "{:?}", fake.changes);
    assert_eq!(adopt(&mut fake, machine, &Target::Machine), SetupExit::Done);
}

#[test]
fn an_install_folder_outside_the_users_profile_changes_nothing() {
    for dir in [
        r"C:\Users\other\AppData\Local\Clauseo",
        r"C:\Users\lawyer",
        r"C:\Program Files\Clauseo",
        r"C:\Users\lawyer\AppData\Local\Missing",
    ] {
        let mut fake = Fake::new();
        fake.folders
            .push(PathBuf::from(r"C:\Users\other\AppData\Local\Clauseo"));
        fake.folders.push(PathBuf::from(r"C:\Users\lawyer"));
        assert_eq!(
            adopt(&mut fake, Path::new(dir), &user()),
            SetupExit::Usage,
            "{dir}"
        );
        assert!(fake.changes.is_empty(), "{dir}: {:?}", fake.changes);
    }
}

#[test]
fn a_per_machine_adopt_records_program_files_clauseo_and_sets_nobody_up() {
    let mut fake = Fake::new();
    // Never asked: no installing user, and a record of anyone's is left as it is.
    fake.owner = Some(Sid::parse(OTHER).unwrap());
    let machine = Path::new(r"C:\Program Files\Clauseo");
    assert_eq!(adopt(&mut fake, machine, &Target::Machine), SetupExit::Done);
    assert_eq!(fake.changes.len(), 2, "{:?}", fake.changes);
    assert!(fake.changes[1].starts_with("record "), "{:?}", fake.changes);
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &Target::Machine),
        SetupExit::Usage
    );
    assert_eq!(fake.changes, Vec::<String>::new());
}
