//! The set-up task on a real machine, through the production registration
//! (`task::register_in`, `task::set_starters_in`) into a test folder: the folder keeps its own
//! descriptor ([`FOLDER_SDDL`]) after a task for another owner is registered in it, the set-up
//! task's descriptor is the one [`Starters`] gives, and a standard user, holding `IU`, may start
//! the set-up task there but not the other owner's stop task, nor the set-up task once its
//! descriptor refuses them.
//!
//! Elevated, from CI's elevated step only (`CLAUSEO_ELEVATED_TESTS=1`): it makes a standard
//! account (`ClauseoSbTT` and six hex digits), a test folder `\ClauseoTestSetUp<pid>` and two
//! LOCAL SYSTEM tasks in it whose action is `cmd.exe`. The account runs `schtasks` through the
//! Secondary Logon service (an interactive logon, so its token holds `IU`), unelevated.
//! Everything is removed on drop, a failed assertion included.
#![expect(unsafe_code, reason = "FFI to the Task Scheduler")]

use std::path::PathBuf;

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::System::TaskScheduler::ITaskService;
use windows::core::BSTR;

use super::staging::as_read_back;
use super::task::{Apartment, register_in, service, set_starters_in};
use super::windows_tests::{system32, try_in_cmd};
use super::{FOLDER_SDDL, Starters, SystemTask, sddl_matches};
use crate::test_support::TempDir;
use crate::windows::record::current_user_sid;
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::test_dacl::{elevated, protect_with};

/// Whether the test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// The test folder: its tasks, then itself, deleted on drop.
struct Folder<'a> {
    service: &'a ITaskService,
    name: String,
}

impl Folder<'_> {
    /// The owner and DACL of `\<name>`, or of the task `task` in it, as Windows reads them back.
    fn read_back(&self, task: Option<SystemTask>) -> String {
        let info = (OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0).cast_signed();
        // SAFETY: a live service; BSTRs that outlive each call; then a live folder or task.
        unsafe {
            let folder = self
                .service
                .GetFolder(&BSTR::from(format!("\\{}", self.name).as_str()))
                .unwrap();
            match task {
                Some(task) => folder
                    .GetTask(&BSTR::from(task.name()))
                    .unwrap()
                    .GetSecurityDescriptor(info),
                None => folder.GetSecurityDescriptor(info),
            }
        }
        .unwrap()
        .to_string()
    }
}

impl Drop for Folder<'_> {
    fn drop(&mut self) {
        // SAFETY: a live service; BSTRs that outlive each call; flags 0.
        unsafe {
            if let Ok(folder) = self
                .service
                .GetFolder(&BSTR::from(format!("\\{}", self.name).as_str()))
            {
                for task in [SystemTask::SetUp, SystemTask::Stop] {
                    drop(folder.DeleteTask(&BSTR::from(task.name()), 0));
                }
            }
            if let Ok(root) = self.service.GetFolder(&BSTR::from("\\"))
                && let Err(error) = root.DeleteFolder(&BSTR::from(self.name.as_str()), 0)
            {
                eprintln!("\\{} wasn't deleted: {error}", self.name);
            }
        }
    }
}

/// Whether `found`, read back, is `expected` as Windows writes it.
fn same(found: &str, expected: &str) -> bool {
    sddl_matches(found, &as_read_back(expected).unwrap())
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_standard_user_can_start_the_set_up_task_beside_another_owners_stop_task() {
    if !gate("elevated_a_standard_user_can_start_the_set_up_task_beside_another_owners_stop_task") {
        return;
    }
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let dir = TempDir::new("set-up-task-run");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    // The stop task's owner: this elevated account, never the test user.
    let other = current_user_sid().unwrap();
    let exe = PathBuf::from(format!(r"{}\cmd.exe", system32()));

    // Dropped last: COM stays up until every interface below is released.
    let _apartment = Apartment::enter().unwrap();
    let service = service().unwrap();
    let folder = Folder {
        service: &service,
        name: format!("ClauseoTestSetUp{}", std::process::id()),
    };
    // The stop task last, as a per-machine set-up does after --adopt registered the set-up task:
    // the folder must not take its descriptor.
    let interactive = Starters::Interactive { denied: None };
    register_in(&folder.name, SystemTask::SetUp, &interactive, &exe).unwrap();
    register_in(
        &folder.name,
        SystemTask::Stop,
        &Starters::Owner(other),
        &exe,
    )
    .unwrap();
    let folder_sd = folder.read_back(None);
    let set_up_sd = folder.read_back(Some(SystemTask::SetUp));
    eprintln!("the folder: {folder_sd}\nthe set-up task: {set_up_sd}");
    assert!(same(&folder_sd, FOLDER_SDDL), "{folder_sd}");
    assert!(same(&set_up_sd, &interactive.sddl()), "{set_up_sd}");

    let schtasks = format!(r"{}\schtasks.exe", system32());
    let run = |task: SystemTask| {
        let line = format!(r#""{schtasks}" /run /tn \{}\{}"#, folder.name, task.name());
        let (code, printed) = try_in_cmd(&user, dir.path(), &line);
        eprintln!("as the test user: {line}\nexit {code:?}\n{printed}");
        code
    };
    // The other owner's stop task: refused. The set-up task, through IU: started.
    assert_ne!(run(SystemTask::Stop), Some(0), "another owner's stop task");
    assert_eq!(
        run(SystemTask::SetUp),
        Some(0),
        "the set-up task, as an interactive user"
    );

    // Once its descriptor refuses a group the user is in (here the user itself), refused.
    let denied = Starters::Interactive {
        denied: Some(user.sid.clone()),
    };
    set_starters_in(&folder.name, SystemTask::SetUp, &denied).unwrap();
    let denied_sd = folder.read_back(Some(SystemTask::SetUp));
    eprintln!("the set-up task, the user refused: {denied_sd}");
    assert!(same(&denied_sd, &denied.sddl()), "{denied_sd}");
    assert_ne!(run(SystemTask::SetUp), Some(0), "refused by the deny");
    // The descriptor change left the folder's own.
    assert!(same(&folder.read_back(None), FOLDER_SDDL));
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_ensure_task_keeps_a_current_task_and_registers_a_changed_one_again() {
    use windows::Win32::Foundation::VARIANT_BOOL;
    use windows::Win32::System::TaskScheduler::{TASK_INSTANCES_IGNORE_NEW, TASK_INSTANCES_QUEUE};

    use super::task::with_task_in;
    use super::task_check::ensure_in;
    use super::{INSTANCES_IGNORE_NEW, INSTANCES_QUEUE};

    assert_eq!(INSTANCES_QUEUE, TASK_INSTANCES_QUEUE.0);
    assert_eq!(INSTANCES_IGNORE_NEW, TASK_INSTANCES_IGNORE_NEW.0);
    if !gate("elevated_ensure_task_keeps_a_current_task_and_registers_a_changed_one_again") {
        return;
    }
    let exe = PathBuf::from(format!(r"{}\cmd.exe", system32()));
    let _apartment = Apartment::enter().unwrap();
    let service = service().unwrap();
    let folder = Folder {
        service: &service,
        name: format!("ClauseoTestEnsure{}", std::process::id()),
    };
    let starters = Starters::Interactive { denied: None };
    let ensure = || ensure_in(&folder.name, SystemTask::SetUp, &starters, &exe);
    // Missing: registered. Then read back as written: current, not registered again.
    assert_eq!(ensure(), Ok(true));
    assert_eq!(
        ensure(),
        Ok(false),
        "a fresh registration reads back current"
    );
    // Disabled by hand: registered again, then current.
    with_task_in(&folder.name, SystemTask::SetUp, |task| {
        // SAFETY: a live registered task; a plain value.
        unsafe { task.SetEnabled(VARIANT_BOOL::from(false)) }.unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(ensure(), Ok(true), "a disabled task is registered again");
    assert_eq!(ensure(), Ok(false));
}
