//! The two SYSTEM tasks' definitions (`rules.rs`), on every OS: the stop task is the refresh
//! task with its own name, argument and limit, the same principal and descriptor, and no logon
//! start; the uninstaller's removal takes both down before the copy they run.

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
}

#[test]
fn both_tasks_take_the_one_descriptor_the_owner_may_only_read_and_run() {
    // `register_task` passes `task_sddl(owner)` to the folder and to every task alike: no task
    // takes a descriptor of its own. The owner reads and runs (FRFX); it can't write, delete or
    // change the DACL, and no one else but SYSTEM and Administrators is named.
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let sddl = task_sddl(&owner);
    assert_eq!(
        sddl,
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    let aces: Vec<&str> = sddl.split('(').skip(1).collect();
    assert_eq!(aces.len(), 3);
    assert!(aces[2].starts_with("A;;FRFX;;;"), "{}", aces[2]);
}

#[test]
fn the_removal_takes_both_tasks_down_after_the_values_and_before_the_copy() {
    assert_eq!(
        TEARDOWN,
        [
            Teardown::AdminValues,
            Teardown::Task(SystemTask::Refresh),
            Teardown::Task(SystemTask::Stop),
            Teardown::Staging,
            Teardown::AdminFolder,
        ]
    );
    for task in TASKS {
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
