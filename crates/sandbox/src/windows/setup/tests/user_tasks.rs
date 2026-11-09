//! A per-user install's stop and sweep tasks (`user_tasks::ensure`): the set-up the app runs as
//! its repair registers a missing task and re-registers one that differs in any part `--adopt`
//! registers, with the path it checked, and leaves the same task alone. A program that fails its
//! check, a task that can't be read or one that can't be registered never rolls the pool back:
//! it is named in the report, the run's result isn't a success (`TASKS_NOT_PUT_BACK`), and the
//! readiness check then says not ready.

use std::path::PathBuf;

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, rid};
use super::fake_more::{TASK_EXE, registered_task};
use super::*;
use crate::windows::refresh::task_identity::{
    AccountName, Carried, INSTANCES_IGNORE_NEW, OnBattery, REFRESH_TASK_MISSING, ReadError,
    TASK_CHANGED, TASK_MISSING, TASK_UNCHECKED, TaskAction, TaskIdentity, TaskReader, TaskTrigger,
    ready_with_tasks, test_descriptor,
};
use crate::windows::refresh::{SystemTask, USER_TASKS};
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::setup::result_file::{self, SetupResult};
use crate::{SetupError, SetupState};

/// A per-user install whose admin copy the owner placed, with its three tasks as `--adopt` left
/// them.
fn adopted(test: &str) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = Some(CopyOwner::User(owner()));
    for task in USER_TASKS {
        let found = Ok(registered_task(task, &owner()));
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

/// Whether `machine` holds `task` as the owner's set-up registers it.
fn holds(machine: &FakeMachine, task: SystemTask) -> bool {
    machine.state.tasks.get(task.name()) == Some(&Ok(registered_task(task, &owner())))
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
    fn found(&mut self, task: SystemTask) -> Result<Option<TaskIdentity>, ReadError> {
        self.0.state.tasks.get(task.name()).cloned().transpose()
    }
    fn descriptor(&mut self, owner: &Sid) -> Result<Vec<u8>, String> {
        Ok(test_descriptor(owner))
    }
    fn account_name(&mut self, sid: &Sid) -> Result<Option<AccountName>, String> {
        let named = (*sid == owner()).then(|| (OWNER_NAME.to_owned(), "PC-7".to_owned()));
        Ok(named)
    }
}

/// The readiness check's state for `machine` once set up.
fn readiness(machine: &FakeMachine) -> SetupState {
    ready_with_tasks(SetupState::Ready, &mut Reader(machine))
}

#[test]
fn a_missing_task_is_registered_again_by_the_set_up() {
    // A deleted task, or the whole `\Clauseo` folder gone (`task_identity::not_found`).
    let mut machine = adopted("user-tasks-missing");
    machine.state.tasks.clear();
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert_eq!(report.tasks, Ok(()));
    let owner_rid = rid(&owner());
    assert_eq!(
        replaced(&machine, 0),
        [
            format!("replace_task SandboxRefresh {owner_rid}"),
            format!("replace_task SandboxStop {owner_rid}"),
            format!("replace_task SandboxSweep {owner_rid}")
        ]
    );
    for task in USER_TASKS {
        assert!(holds(&machine, task), "{task:?}");
    }
    assert_eq!(readiness(&machine), SetupState::Ready);
    // Idempotent: a re-run finds every task the same and touches none.
    let from = machine.calls.len();
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(replaced(&machine, from).is_empty(), "{:?}", machine.calls);
    assert!(machine.calls_since(from).contains(&"task_exe"));
}

/// A part's name, and its change to a task as read back.
type Change = (&'static str, fn(&mut TaskIdentity));

#[test]
fn a_task_differing_in_any_part_is_registered_again() {
    let changes: [Change; 11] = [
        ("principal", |t| t.principal = OTHER_OWNER.to_owned()),
        ("action", |t| {
            t.actions = vec![TaskAction::Exec {
                path: r#""C:\Users\lawyer\AppData\Local\Clauseo\evil.exe""#.to_owned(),
                arguments: "--stop".to_owned(),
            }];
        }),
        ("another action", |t| t.actions.push(TaskAction::Other)),
        ("arguments", |t| {
            if let Some(TaskAction::Exec { arguments, .. }) = t.actions.first_mut() {
                arguments.push_str(" --refresh");
            }
        }),
        ("trigger", |t| t.triggers.push(TaskTrigger::Other(1))),
        ("disabled", |t| t.settings.enabled = false),
        ("demand start", |t| t.settings.demand_start = false),
        ("batteries", |t| t.settings.on_battery = OnBattery::NoStart),
        ("instances", |t| {
            t.settings.instances = INSTANCES_IGNORE_NEW + 1;
        }),
        ("run level", |t| t.run_level = 0),
        ("descriptor", |t| {
            t.descriptor = test_descriptor(&Sid::parse(OTHER_OWNER).unwrap());
        }),
    ];
    for (part, change) in changes {
        let mut machine = adopted(&format!("user-tasks-differs-{}", part.replace(' ', "-")));
        if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
            change(stop);
        }
        let changed = SetupState::Failed(TASK_CHANGED.to_owned());
        assert_eq!(readiness(&machine), changed, "{part}");
        set_up_fresh(&mut machine, &mut FakeAcl::default());
        let call = format!("replace_task SandboxStop {}", rid(&owner()));
        assert_eq!(replaced(&machine, 0), [call], "{part}");
        assert!(holds(&machine, SystemTask::Stop), "{part}");
        assert_eq!(readiness(&machine), SetupState::Ready, "{part}");
    }
    // A task the set-up may not read is a changed descriptor: registered again.
    let mut machine = adopted("user-tasks-refused");
    machine
        .state
        .tasks
        .insert("SandboxSweep".to_owned(), Err(ReadError::Refused));
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(replaced(&machine, 0).len(), 1, "{:?}", machine.calls);
    assert_eq!(readiness(&machine), SetupState::Ready);
}

#[test]
fn a_program_that_fails_its_check_registers_nothing_and_stays_not_ready() {
    // `task_exe` fails as `admin_copy::check` does for a writable copy or folder above it, a
    // planted file or a changed hash (`task_identity_tests` holds the check to each).
    let mut machine = adopted("user-tasks-writable");
    machine.state.tasks.clear();
    machine.fail("task_exe");
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    let why = report.tasks.unwrap_err();
    assert!(why.contains("failed its check"), "{why}");
    assert!(why.contains("injected"), "{why}");
    assert!(replaced(&machine, 0).is_empty(), "{:?}", machine.calls);
    assert!(machine.state.tasks.is_empty());
    // The pool is set up all the same; only the tasks wait for the next repair.
    assert!(machine.state.record.is_some());
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
}

#[test]
fn one_task_that_cant_be_read_or_registered_spares_the_pool_and_the_other_task() {
    let mut machine = adopted("user-tasks-unread");
    let down = ReadError::Failed("ITaskService::Connect failed".to_owned());
    machine
        .state
        .tasks
        .insert("SandboxStop".to_owned(), Err(down));
    machine.state.tasks.remove("SandboxSweep");
    // Every task is checked, and the missing one said over the unreadable one.
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
    machine.state.tasks.insert(
        "SandboxSweep".to_owned(),
        Ok(registered_task(SystemTask::Sweep, &owner())),
    );
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_UNCHECKED.to_owned())
    );
    machine.state.tasks.remove("SandboxSweep");
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    let why = report.tasks.unwrap_err();
    assert!(why.contains("SandboxStop can't be read"), "{why}");
    let owner_rid = rid(&owner());
    assert_eq!(
        replaced(&machine, 0),
        [format!("replace_task SandboxSweep {owner_rid}")]
    );
    // A registration that fails is named; the set-up still succeeds.
    let mut machine = adopted("user-tasks-replace-fails");
    machine.state.tasks.clear();
    machine.fail(&format!("replace_task SandboxStop {owner_rid}"));
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    let why = report.tasks.unwrap_err();
    assert!(why.contains("SandboxStop"), "{why}");
    assert!(!why.contains("SandboxSweep"), "{why}");
    assert!(holds(&machine, SystemTask::Sweep));
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
}

#[test]
fn readiness_says_missing_over_changed_whichever_task_comes_first() {
    // The VM's case: the stop task, checked first, read as changed, while the sweep task was
    // gone. The repair puts both back either way; the words say the most it can do.
    let mut machine = adopted("user-tasks-order");
    if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
        stop.settings.enabled = false;
    }
    machine.state.tasks.remove("SandboxSweep");
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(readiness(&machine), SetupState::Ready);
}

#[test]
fn the_refresh_task_is_put_back_like_the_others() {
    // A deleted `\Clauseo` folder takes the refresh task too: the repair registers it again,
    // for the owner's logon. Until then readiness offers the repair without refusing anything.
    let mut machine = adopted("user-tasks-refresh");
    machine.state.tasks.remove("SandboxRefresh");
    assert_eq!(
        readiness(&machine),
        SetupState::RepairNeeded(REFRESH_TASK_MISSING.to_owned())
    );
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(report.tasks, Ok(()));
    let owner_rid = rid(&owner());
    assert_eq!(
        replaced(&machine, 0),
        [format!("replace_task SandboxRefresh {owner_rid}")]
    );
    assert!(holds(&machine, SystemTask::Refresh));
    assert_eq!(readiness(&machine), SetupState::Ready);
}

#[test]
fn an_earlier_adopts_task_without_a_definition_descriptor_is_left_alone() {
    // A task an earlier --adopt registered, before registration set the definition's own
    // descriptor: the task's own descriptor is the set-up's, so it is the same and left.
    let mut machine = adopted("user-tasks-no-sd");
    if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
        stop.definition_descriptor = Carried::Absent;
    }
    assert_eq!(readiness(&machine), SetupState::Ready);
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert!(replaced(&machine, 0).is_empty(), "{:?}", machine.calls);
    // With its own descriptor wrong too, it is registered again, and what the registration
    // writes carries the descriptor it was given.
    let mut machine = adopted("user-tasks-no-sd-wrong");
    if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
        stop.definition_descriptor = Carried::Absent;
        stop.descriptor = test_descriptor(&Sid::parse(OTHER_OWNER).unwrap());
    }
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_CHANGED.to_owned())
    );
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    let owner_rid = rid(&owner());
    assert_eq!(
        replaced(&machine, 0),
        [format!("replace_task SandboxStop {owner_rid}")]
    );
    let registered = machine.state.tasks.get("SandboxStop").cloned();
    let carried = registered
        .and_then(Result::ok)
        .map(|t| t.definition_descriptor);
    let given = Carried::Descriptor(test_descriptor(&owner()));
    assert_eq!(carried, Some(given));
    assert_eq!(readiness(&machine), SetupState::Ready);
}

#[test]
fn a_task_updated_in_place_with_an_older_descriptor_gets_the_set_ups() {
    // An earlier build's task: registering it again updates it in place, which keeps its own
    // descriptor (here one naming another owner), so the repair then sets that descriptor.
    let owner_rid = rid(&owner());
    let older = test_descriptor(&Sid::parse(OTHER_OWNER).unwrap());
    let mut machine = adopted("user-tasks-heal");
    if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
        stop.descriptor = older.clone();
    }
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_CHANGED.to_owned())
    );
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(report.tasks, Ok(()));
    let tasks = ["replace_task", "heal_task"];
    let calls = machine.calls_since(0);
    let calls = calls
        .into_iter()
        .filter(|c| tasks.iter().any(|task| c.starts_with(task)));
    let calls: Vec<&str> = calls.collect();
    let stop = |call: &str| format!("{call} SandboxStop {owner_rid}");
    assert_eq!(calls, [stop("replace_task"), stop("heal_task_descriptor")]);
    assert!(holds(&machine, SystemTask::Stop));
    assert_eq!(readiness(&machine), SetupState::Ready);
    // One the set-up can't set is named, and readiness still says changed.
    let mut machine = adopted("user-tasks-heal-fails");
    if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
        stop.descriptor = older;
    }
    machine.fail(&stop("heal_task_descriptor"));
    let why = set_up_fresh(&mut machine, &mut FakeAcl::default())
        .tasks
        .unwrap_err();
    assert!(why.contains("SandboxStop"), "{why}");
    assert_eq!(
        readiness(&machine),
        SetupState::Failed(TASK_CHANGED.to_owned())
    );
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

/// What the app is told about `report`: the exit code, the result file the exe writes, and
/// what `UserSetup`'s repair returns from them (`result_file::outcome`).
fn told(report: SetupReport) -> (SetupExit, SetupResult, Result<(), SetupError>) {
    let outcome = Ok(report);
    let exit = exit_of(&outcome);
    let result = SetupResult::of(&outcome);
    let app = result_file::outcome(u32::from(exit.as_code()), Some(&Ok(Some(result.clone()))));
    (exit, result, app)
}

#[test]
fn a_repair_that_cant_put_a_task_back_isnt_a_success_but_keeps_the_pool() {
    let owner_rid = rid(&owner());
    let missing = SetupState::Failed(TASK_MISSING.to_owned());
    let changed = SetupState::Failed(TASK_CHANGED.to_owned());
    for (test, tamper, after) in [
        ("user-tasks-repair-missing", false, missing),
        ("user-tasks-repair-changed", true, changed),
    ] {
        let mut machine = adopted(test);
        if tamper {
            if let Some(Ok(stop)) = machine.state.tasks.get_mut("SandboxStop") {
                stop.settings.enabled = false;
            }
        } else {
            machine.state.tasks.remove("SandboxStop");
        }
        machine.fail(&format!("replace_task SandboxStop {owner_rid}"));
        let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
        assert!(report.tasks.is_err(), "{test}");
        // The pool's set-up stays applied: users, record, marker and the other task.
        assert_eq!(report.pool.len(), MIN_FREE_AFTER_RUN, "{test}");
        assert!(machine.state.record.is_some(), "{test}");
        assert!(machine.state.marker.is_some(), "{test}");
        assert!(holds(&machine, SystemTask::Sweep), "{test}");
        let (exit, result, app) = told(report);
        assert_eq!(exit, SetupExit::Failed, "{test}");
        assert_eq!(result.exit, 3, "{test}");
        assert_eq!(
            result.step.as_deref(),
            Some(Step::StopTask.to_string().as_str())
        );
        assert_eq!(result.cause.as_deref(), Some(TASKS_NOT_PUT_BACK), "{test}");
        assert_eq!(
            app,
            Err(SetupError::Failed(TASKS_NOT_PUT_BACK.to_owned())),
            "{test}"
        );
        assert_eq!(readiness(&machine), after, "{test}");
    }
}

#[test]
fn a_repair_that_puts_every_task_back_is_a_success() {
    let mut machine = adopted("user-tasks-repair-both");
    machine.state.tasks.clear();
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(report.tasks, Ok(()));
    let (exit, result, app) = told(report);
    assert_eq!(exit, SetupExit::Done);
    assert_eq!((result.exit, result.step, result.cause), (0, None, None));
    assert_eq!(app, Ok(()));
    assert_eq!(readiness(&machine), SetupState::Ready);
}
