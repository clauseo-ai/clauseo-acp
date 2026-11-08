//! The SYSTEM tasks' definitions (`rules.rs`), on every OS: the stop task is the refresh task
//! with its own name, argument and limit, the same principal and descriptor, and no logon start;
//! the set-up task queues no runs and is started by the owner or by interactive users but the
//! pool's group; the folder keeps its own descriptor; the uninstaller's removal takes every task
//! down before the copy they run, the set-up task first.

use super::*;
use crate::windows::setup::args::{Mode, parse};

#[test]
fn the_stop_task_is_the_refresh_task_but_for_its_name_argument_and_limit() {
    assert_eq!(TASKS, [SystemTask::Refresh, SystemTask::Stop]);
    let refresh = SystemTask::Refresh;
    let stop = SystemTask::Stop;
    assert_eq!(
        (
            refresh.name(),
            refresh.argument(),
            refresh.execution_limit()
        ),
        ("SandboxRefresh", "--refresh", "PT5M")
    );
    assert_eq!(
        (stop.name(), stop.argument(), stop.execution_limit()),
        ("SandboxStop", "--stop", "PT2M")
    );
    assert_eq!((STOP_ARG, STOP_EXECUTION_LIMIT), ("--stop", "PT2M"));
    // One fixed argument each, with no value and nothing to quote: the action passes nothing
    // a caller chose.
    for task in TASKS {
        let argument = task.argument();
        assert!(argument.starts_with("--"), "{argument}");
        assert!(!argument.contains([' ', '=', '"', '\t']), "{argument}");
        assert_ne!(task.description(), "");
    }
    // The refresh also starts at the owner's logon; the stop only when asked.
    assert!(refresh.at_logon());
    assert!(!stop.at_logon());
    // The argument each task passes is the one the exe parses as its mode, and alone.
    let mode = |task: SystemTask| parse([std::ffi::OsString::from(task.argument())]);
    assert_eq!(mode(refresh), Ok(Mode::Refresh));
    assert_eq!(mode(stop), Ok(Mode::Stop));
    let set_up = SystemTask::SetUp;
    assert_eq!(
        (set_up.name(), set_up.argument(), set_up.execution_limit()),
        ("SandboxSetUp", "--serve-set-up", "PT30M")
    );
    assert_eq!(mode(set_up), Ok(Mode::ServeSetUp));
    assert!(!set_up.at_logon(), "started only on demand: no trigger");
    // No run queues: a start during a set-up run is dropped, and the app starts it again.
    assert!(!set_up.queues() && !refresh.queues() && !stop.queues());
    assert_ne!(set_up.description(), "");
}

#[test]
fn the_set_up_task_is_the_owners_per_user_and_interactive_users_but_the_pool_per_machine() {
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let group = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1050").unwrap();
    assert_eq!(Starters::Owner(owner.clone()).sddl(), task_sddl(&owner));
    assert_eq!(Starters::Owner(owner.clone()).owner(), Some(&owner));
    let install = Starters::Interactive { denied: None };
    assert_eq!(
        install.sddl(),
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;IU)"
    );
    assert_eq!(install.owner(), None);
    // After the first set-up: the group's deny comes first, so it wins over IU's grant.
    let denied = Starters::Interactive {
        denied: Some(group),
    };
    assert_eq!(
        denied.sddl(),
        "O:BAD:P(D;;FRFX;;;S-1-5-21-1-2-3-1050)(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;IU)"
    );
    let sddl = denied.sddl();
    let aces: Vec<&str> = sddl.split('(').skip(1).collect();
    assert!(aces[0].starts_with("D;;FRFX;;;"), "{aces:?}");
    for ace in &aces[3..] {
        for wider in ["FA", "FW", "GA", "GW", "WD", "WO", "SD"] {
            assert!(!ace.contains(wider), "{wider}: {ace}");
        }
    }
}

#[test]
fn the_folder_has_its_own_descriptor_whatever_task_is_registered() {
    // Not any task's: a folder given the stop task's descriptor (one owner) would shut every
    // other interactive user out of the set-up task inside it.
    assert_eq!(FOLDER_SDDL, "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;IU)");
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_ne!(FOLDER_SDDL, task_sddl(&owner));
    assert_ne!(FOLDER_SDDL, Starters::Interactive { denied: None }.sddl());
    // Read alone for IU: no one but SYSTEM and Administrators adds or changes a task in it.
    let aces: Vec<&str> = FOLDER_SDDL.split('(').skip(1).collect();
    assert_eq!(aces.len(), 3);
    assert_eq!(aces[2], "A;;FR;;;IU)");
}

#[test]
fn both_tasks_take_the_one_descriptor_the_owner_may_read_and_execute() {
    // `register_task` passes `task_sddl(owner)` to the folder and to every task alike: no task
    // takes a descriptor of its own. The owner reads and executes (FRFX): starting a task needs
    // execute (CI run 42416074600), which also ends one. It can't write, delete or change the
    // DACL. No one else but SYSTEM and Administrators is named.
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let sddl = task_sddl(&owner);
    assert_eq!(
        sddl,
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    let aces: Vec<&str> = sddl.split('(').skip(1).collect();
    assert_eq!(aces.len(), 3);
    assert!(aces[2].starts_with("A;;FRFX;;;"), "{}", aces[2]);
    for wider in ["FA", "FW", "GA", "GW", "WD", "WO", "SD"] {
        assert!(!aces[2].contains(wider), "{wider}: {}", aces[2]);
    }
}

#[test]
fn the_removal_takes_every_task_down_after_the_values_and_before_the_copy() {
    assert_eq!(
        TEARDOWN,
        [
            Teardown::AdminValues,
            Teardown::Task(SystemTask::SetUp),
            Teardown::Task(SystemTask::Refresh),
            Teardown::Task(SystemTask::Stop),
            Teardown::Staging,
            Teardown::AdminFolder,
        ]
    );
    for task in [SystemTask::SetUp, SystemTask::Refresh, SystemTask::Stop] {
        let at = TEARDOWN
            .iter()
            .position(|step| *step == Teardown::Task(task));
        let at = at.unwrap_or_else(|| panic!("{task:?} isn't taken down"));
        let folder = TEARDOWN
            .iter()
            .position(|step| *step == Teardown::AdminFolder);
        assert!(Some(at) < folder, "{task:?} goes before the copy");
        assert!(at > 0, "{task:?} goes after the values");
    }
}

#[test]
fn the_action_names_the_admin_copy_in_quotes() {
    let exe = Path::new(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe");
    assert_eq!(
        action_path(exe),
        r#""C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe""#
    );
}

#[test]
fn a_missing_task_or_folder_is_not_registered_whichever_code_windows_gives() {
    // ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND as HRESULTs: either leaves the prompt.
    assert!(not_registered(0x8007_0002));
    assert!(not_registered(0x8007_0003));
    // Access denied, and a bare Win32 code, are failures, not a missing task.
    assert!(!not_registered(0x8007_0005));
    assert!(!not_registered(2));
}
