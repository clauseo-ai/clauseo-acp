//! A per-user install's stop and sweep tasks (`user_tasks::ensure`): the set-up the app runs as
//! its repair registers a missing task, replaces one that differs in any part `--adopt`
//! registers, leaves the same task alone, and registers nothing while the program they would run
//! as SYSTEM isn't limited to administrators; the readiness check then says not ready.

use std::path::PathBuf;

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, rid};
use super::fake_more::{TASK_EXE, registered_task};
use super::*;
use crate::SetupState;
use crate::windows::refresh::task_identity::{
    TASK_MISSING, TaskAction, TaskIdentity, TaskReader, TaskTrigger, ready_with_tasks,
};
use crate::windows::refresh::{POOL_TASKS, SystemTask};
use crate::windows::setup::record_values::CopyOwner;

/// A per-user install whose admin copy the owner placed, with both tasks as `--adopt` left them.
fn adopted(test: &str) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = Some(CopyOwner::User(owner()));
    for task in POOL_TASKS {
        let found = registered_task(task, &owner());
        machine.state.tasks.insert(task.name().to_owned(), found);
    }
    machine
}

/// The `replace_task` calls since `from`.
fn replaced(machine: &FakeMachine, from: usize) -> Vec<String> {
    let calls = machine.calls_since(from);
    let calls = calls.into_iter().filter(|c| c.starts_with("replace_task"));
    calls.map(str::to_owned).collect()
}

/// The readiness check over `machine`'s tasks, as the owner.
struct Reader<'a>(&'a FakeMachine);

impl TaskReader for Reader<'_> {
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, String> {
        Ok(self.0.state.copy_owner.clone())
    }
    fn user(&mut self) -> Result<Sid, String> {
        Ok(owner())
    }
    fn exe(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(TASK_EXE))
    }
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, String> {
        Ok(self.0.state.tasks.get(task.name()).cloned())
    }
}

#[test]
fn a_missing_task_is_registered_again_by_the_set_up() {
    let mut machine = adopted("user-tasks-missing");
    machine.state.tasks.remove("SandboxSweep");
    assert_eq!(
        ready_with_tasks(SetupState::Ready, &mut Reader(&machine)),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let owner_rid = rid(&owner());
    assert_eq!(
        replaced(&machine, 0),
        [format!("replace_task SandboxSweep {owner_rid} false")]
    );
    let sweep = machine.state.tasks.get("SandboxSweep");
    assert_eq!(sweep, Some(&registered_task(SystemTask::Sweep, &owner())));
    assert_eq!(
        ready_with_tasks(SetupState::Ready, &mut Reader(&machine)),
        SetupState::Ready
    );
    // Idempotent: a re-run finds both tasks the same and touches neither.
    let from = machine.calls.len();
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(replaced(&machine, from).is_empty(), "{:?}", machine.calls);
    assert!(machine.calls_since(from).contains(&"task_exe"));
}

/// A part's name, and its change to a task as read back.
type Change = (&'static str, fn(&mut TaskIdentity));

#[test]
fn a_task_differing_in_any_part_is_replaced() {
    let other = Sid::parse(OTHER_OWNER).unwrap();
    let changes: [Change; 8] = [
        ("principal", |t| t.principal = OTHER_OWNER.to_owned()),
        ("action", |t| {
            t.actions = vec![TaskAction::Exec {
                path: r#""C:\Users\lawyer\AppData\Local\Clauseo\evil.exe""#.to_owned(),
                arguments: "--stop".to_owned(),
            }];
        }),
        ("another action", |t| t.actions.push(TaskAction::Other)),
        ("no action", |t| t.actions.clear()),
        ("arguments", |t| {
            if let Some(TaskAction::Exec { arguments, .. }) = t.actions.first_mut() {
                arguments.push_str(" --refresh");
            }
        }),
        ("trigger", |t| t.triggers.push(TaskTrigger::Other(1))),
        ("logon trigger", |t| {
            t.triggers.push(TaskTrigger::Logon(String::new()));
        }),
        ("descriptor", |t| {
            t.sddl = crate::windows::refresh::task_sddl(&Sid::parse(OTHER_OWNER).unwrap());
        }),
    ];
    for (part, change) in changes {
        let mut machine = adopted(&format!("user-tasks-differs-{}", part.replace(' ', "-")));
        let stop = machine.state.tasks.get_mut("SandboxStop").unwrap();
        change(stop);
        assert_ne!(stop, &registered_task(SystemTask::Stop, &owner()), "{part}");
        assert_eq!(
            ready_with_tasks(SetupState::Ready, &mut Reader(&machine)),
            SetupState::Failed(TASK_MISSING.to_owned()),
            "{part}"
        );
        let mut acl = FakeAcl::default();
        set_up_fresh(&mut machine, &mut acl);
        assert_eq!(
            replaced(&machine, 0),
            [format!("replace_task SandboxStop {} true", rid(&owner()))],
            "{part}"
        );
        let stop = machine.state.tasks.get("SandboxStop");
        assert_eq!(
            stop,
            Some(&registered_task(SystemTask::Stop, &owner())),
            "{part}"
        );
        assert_eq!(
            ready_with_tasks(SetupState::Ready, &mut Reader(&machine)),
            SetupState::Ready,
            "{part}"
        );
    }
    // A task registered for another owner is another descriptor: replaced too.
    let mut machine = adopted("user-tasks-differs-owner");
    let theirs = registered_task(SystemTask::Sweep, &other);
    machine
        .state
        .tasks
        .insert("SandboxSweep".to_owned(), theirs);
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(replaced(&machine, 0).len(), 1, "{:?}", machine.calls);
}

#[test]
fn a_user_writable_action_program_registers_nothing_and_stays_not_ready() {
    // `task_exe` fails as `admin_copy::check_chain` does for a writable copy or folder above it
    // (`task_identity_tests` holds the chain itself to that).
    let mut machine = adopted("user-tasks-writable");
    machine.state.tasks.clear();
    machine.fail("task_exe");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::StopTask);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(
        failure.cause.contains("isn't limited to"),
        "{}",
        failure.cause
    );
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
    assert!(replaced(&machine, 0).is_empty(), "{:?}", machine.calls);
    assert!(machine.state.tasks.is_empty());
    assert_eq!(
        ready_with_tasks(SetupState::Ready, &mut Reader(&machine)),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
}

#[test]
fn a_failed_replacement_fails_the_set_up_closed() {
    let mut machine = adopted("user-tasks-replace-fails");
    machine.state.tasks.remove("SandboxStop");
    machine.fail(&format!("replace_task SandboxStop {} false", rid(&owner())));
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::StopTask);
    assert!(failure.cause.contains("SandboxStop"), "{}", failure.cause);
}

#[test]
fn another_users_copy_and_a_machine_install_get_no_per_user_tasks() {
    let other = Sid::parse(OTHER_OWNER).unwrap();
    for (test, copy) in [
        ("user-tasks-other", CopyOwner::User(other)),
        ("user-tasks-machine", CopyOwner::Machine),
    ] {
        let mut machine = FakeMachine::new(test);
        machine.state.copy_owner = Some(copy);
        set_up_fresh(&mut machine, &mut FakeAcl::default());
        assert!(replaced(&machine, 0).is_empty(), "{test}");
        assert!(!machine.calls.iter().any(|c| c == "task_exe"), "{test}");
        assert!(machine.state.tasks.is_empty(), "{test}");
    }
}
