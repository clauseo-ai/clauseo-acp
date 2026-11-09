//! `task_keep.rs`'s no-prompt heal, on every OS: `--refresh`, as SYSTEM, puts back only the three
//! fixed tasks, for the owner the record names, with the fixed descriptor, from the checked admin
//! copy, and takes nothing from the lawyer: no argument, no environment, no file of theirs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::fake::*;
use super::task_identity::{AccountName, ReadError, TaskIdentity, test_descriptor};
use super::task_keep::{TaskKeeper, keep_tasks};
use super::test_signer::comment;
use super::*;

/// A machine's tasks as SYSTEM sees them, and every effect, as a line.
#[derive(Default)]
struct Keeper {
    tasks: BTreeMap<&'static str, TaskIdentity>,
    /// Why the admin copy fails its check, when it does.
    blocked: Option<String>,
    /// The task whose registration fails.
    fails: Option<&'static str>,
    calls: Vec<String>,
}

impl Keeper {
    /// The shipped per-user install: the refresh and stop tasks, no sweep task.
    fn shipped(owner: &Sid) -> Keeper {
        let mut keeper = Keeper::default();
        for task in [SystemTask::Refresh, SystemTask::Stop] {
            let found =
                TaskIdentity::expected(task, owner, Path::new(ADMIN), test_descriptor(owner));
            keeper.tasks.insert(task.name(), found);
        }
        keeper
    }
}

impl TaskKeeper for Keeper {
    fn exe(&mut self) -> Result<PathBuf, String> {
        self.calls.push("check the admin copy".to_owned());
        self.blocked
            .clone()
            .map_or_else(|| Ok(PathBuf::from(ADMIN)), Err)
    }
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        Ok(self.tasks.get(task.name()).cloned())
    }
    fn expected(
        &mut self,
        task: SystemTask,
        owner: &Sid,
        exe: &Path,
    ) -> Result<TaskIdentity, String> {
        Ok(TaskIdentity::expected(
            task,
            owner,
            exe,
            test_descriptor(owner),
        ))
    }
    fn register(&mut self, task: SystemTask, owner: &Sid, exe: &Path) -> Result<(), String> {
        self.calls
            .push(format!("register {} {}", task.name(), owner.as_str()));
        if self.fails == Some(task.name()) {
            return Err("RegisterTaskDefinition failed".to_owned());
        }
        let found = TaskIdentity::expected(task, owner, exe, test_descriptor(owner));
        self.tasks.insert(task.name(), found);
        Ok(())
    }
    fn set_descriptor(&mut self, task: SystemTask, owner: &Sid) -> Result<(), String> {
        self.calls
            .push(format!("set descriptor {} {}", task.name(), owner.as_str()));
        Ok(())
    }
    fn account_name(&mut self, _sid: &Sid) -> Result<Option<AccountName>, String> {
        Ok(None)
    }
}

fn owner() -> Sid {
    Sid::parse(OWNER).unwrap()
}

#[test]
fn the_shipped_install_gets_its_sweep_task_and_nothing_else() {
    let mut keeper = Keeper::shipped(&owner());
    assert_eq!(keep_tasks(&mut keeper, &owner()), Ok(vec!["SandboxSweep"]));
    let registered: Vec<&String> = keeper
        .calls
        .iter()
        .filter(|c| c.starts_with("register"))
        .collect();
    assert_eq!(registered, [&format!("register SandboxSweep {OWNER}")]);
    // Run again: nothing to put back.
    assert_eq!(keep_tasks(&mut keeper, &owner()), Ok(vec![]));
    // With all three gone, only the three fixed names, ever.
    let mut keeper = Keeper::default();
    assert_eq!(
        keep_tasks(&mut keeper, &owner()),
        Ok(vec!["SandboxRefresh", "SandboxStop", "SandboxSweep"])
    );
    let names: Vec<&str> = keeper.tasks.keys().copied().collect();
    assert_eq!(names, ["SandboxRefresh", "SandboxStop", "SandboxSweep"]);
    assert_eq!(
        USER_TASKS.map(SystemTask::name),
        ["SandboxRefresh", "SandboxStop", "SandboxSweep"]
    );
}

#[test]
fn a_copy_that_fails_its_check_registers_nothing_and_a_failure_is_named() {
    let mut keeper = Keeper::shipped(&owner());
    keeper.blocked = Some("AdminCopySha256 doesn't match".to_owned());
    let why = keep_tasks(&mut keeper, &owner()).unwrap_err();
    assert!(why.contains("failed its check"), "{why}");
    assert_eq!(keeper.calls, ["check the admin copy"]);
    let mut keeper = Keeper::shipped(&owner());
    keeper.fails = Some("SandboxSweep");
    let why = keep_tasks(&mut keeper, &owner()).unwrap_err();
    assert!(why.contains("SandboxSweep"), "{why}");
}

#[test]
fn the_refresh_keeps_the_tasks_of_the_records_owner_alone() {
    let key = key();
    // A run that left the copy current, or one that refused the new exe, keeps the tasks of the
    // record's AdminCopyOwner; SYSTEM reads nothing else for it.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.keep = Ok(vec!["SandboxSweep"]);
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.kept_for, [owner()]);
    assert!(
        fake.logs
            .iter()
            .any(|(_, line)| line.contains("SandboxSweep"))
    );
    // A signature naming another user changes nothing: the owner is the record's.
    let other = "S-1-5-21-1-2-3-1002";
    let mut fake = Fake::new(&old(), &new(), &key, &comment(other, OWN));
    let _refused = run_fake(&mut fake, &key);
    assert_eq!(fake.kept_for, [owner()]);
    // A per-machine install, a run that isn't SYSTEM's from the admin copy, and a record that
    // can't be read keep nothing.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.record.copy_owner = Some("machine".to_owned());
    fake.admin_only = true;
    let _done = run_fake(&mut fake, &key);
    assert_eq!(fake.kept_for, Vec::<Sid>::new());
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.system = false;
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::NotAllowed);
    assert_eq!(fake.kept_for, Vec::<Sid>::new());
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.record.copy_owner = None;
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::NoRecord);
    assert_eq!(fake.kept_for, Vec::<Sid>::new());
}

#[test]
fn the_heal_takes_no_input_from_the_lawyer() {
    // No argument: `--refresh` with anything after it is a usage error (`setup::args`).
    use std::ffi::OsString;

    use crate::windows::setup::args::{Mode, parse};
    let parse_mode_for_tests =
        |line: &[&str]| parse(line.iter().map(OsString::from)).is_ok_and(|m| m == Mode::Refresh);
    assert!(parse_mode_for_tests(&["--refresh"]));
    for extra in [
        "S-1-5-21-1-2-3-1002",
        "--owner",
        r"C:\Users\Rohan\owner.sid",
    ] {
        assert!(!parse_mode_for_tests(&["--refresh", extra]), "{extra}");
    }
    // No environment, no file of the lawyer's, nothing under HKCU, in any of the code the heal
    // runs: the refresh, the keeper and the registration read none of them.
    let sources = [
        include_str!("../refresh.rs"),
        include_str!("task_keep.rs"),
        include_str!("windows_refresher.rs"),
        include_str!("task_register.rs"),
        include_str!("task_read.rs"),
    ];
    for source in sources {
        for read in [
            "env::var",
            "var_os(",
            "HKEY_CURRENT_USER",
            "FOLDERID_Profile",
            "USERPROFILE",
        ] {
            assert!(!source.contains(read), "{read}");
        }
    }
}
