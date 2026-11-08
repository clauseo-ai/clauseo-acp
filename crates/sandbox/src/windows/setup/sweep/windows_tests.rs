//! The sweep's task step on a real machine (`windows_sweeper.rs`): a test user's scheduled task,
//! made as that user, is deleted when the pool is that user's SID, and the task this process's
//! user (the lawyer, here an elevated administrator) made beside it is left. And this machine's
//! own tasks all have a principal that reads and resolves, so a real run doesn't end 55 on a
//! clean machine.
//!
//! The gate is skipped (it needs LOCAL SYSTEM and a `ClauseoSbNN` pool), as the stop's BITS test
//! skips it: the step is called with the pool the test chose. Elevated, as CI's elevated step
//! runs it (`CLAUSEO_ELEVATED_TESTS=1`): making an account needs it.

use std::process::Command;

use super::windows_sweeper::WindowsSweeper;
use super::{Names, Resolved, Sweeper as _, Task, resolve, sweep_tasks};
use crate::test_support::TempDir;
use crate::windows::refresh::windows_tests::{system32, try_in_cmd};
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

/// The tasks the test made, deleted on drop as this administrator, whoever made them; one
/// already gone is no error.
struct Made(Vec<String>);

impl Drop for Made {
    #[expect(
        clippy::disallowed_methods,
        reason = "the clean-up of the test's own tasks, through schtasks"
    )]
    fn drop(&mut self) {
        for name in &self.0 {
            let deleted = Command::new(format!(r"{}\schtasks.exe", system32()))
                .args(["/delete", "/tn", name, "/f"])
                .output();
            if let Ok(output) = deleted
                && output.status.success()
            {
                eprintln!("the clean-up deleted the task {name}");
            }
        }
    }
}

/// The root folder's tasks, as the sweep reads them.
fn root_tasks(sweeper: &mut WindowsSweeper) -> Vec<Task> {
    let root = sweeper.root().unwrap();
    sweeper.tasks(&root).unwrap()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "makes the lawyer's own task through schtasks, as this process"
)]
fn elevated_the_sweep_deletes_a_pool_users_task_and_leaves_the_lawyers() {
    if !gate("elevated_the_sweep_deletes_a_pool_users_task_and_leaves_the_lawyers") {
        return;
    }
    // 1. A standard account, as the stop's BITS test makes one.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let sid = user.sid.clone();
    eprintln!("1. the test user: {} {}", user.name, sid.as_str());
    let dir = TempDir::new("sweep-tasks");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let suffix = format!("{}{}", std::process::id(), random_hex(2));
    let theirs = format!("ClauseoSweepPool{suffix}");
    let mine = format!("ClauseoSweepLawyer{suffix}");
    // Dropped before the user, so the account outlives its task's clean-up.
    let _made = Made(vec![theirs.clone(), mine.clone()]);
    let schtasks = format!(r"{}\schtasks.exe", system32());
    let action = format!(r"{}\cmd.exe /c exit 0", system32());

    // 2. As that user, unelevated: a task of its own, in the root folder.
    let create =
        format!(r#""{schtasks}" /create /tn {theirs} /tr "{action}" /sc once /st 23:59 /f"#);
    let (code, printed) = try_in_cmd(&user, dir.path(), &create);
    eprintln!("2. as the test user: {create}\nexit {code:?}\n{printed}");
    assert_eq!(
        code,
        Some(0),
        "schtasks /create as the test user:\n{printed}"
    );

    // 3. As this process, the lawyer: a task beside it.
    let output = Command::new(&schtasks)
        .args(["/create", "/tn", &mine, "/tr", &action])
        .args(["/sc", "once", "/st", "23:59", "/f"])
        .output()
        .unwrap();
    eprintln!("3. as this process: {:?}", output.status);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 4. Both are read, and each principal resolves: the test user's to its SID, the lawyer's
    // to another.
    let mut sweeper = WindowsSweeper::new();
    let listed = root_tasks(&mut sweeper);
    let names = Names::of(&mut sweeper, std::slice::from_ref(&sid));
    assert_eq!(
        names.users.len(),
        1,
        "the test user's name is read: {names:?}"
    );
    for (name, pool) in [(&theirs, true), (&mine, false)] {
        let task = listed.iter().find(|task| &task.name == name).unwrap();
        let principal = task.principal.clone().unwrap();
        eprintln!("4. {name}: {principal:?}");
        let named = principal.name().unwrap();
        let Resolved::Sid(resolved) = resolve(&mut sweeper, &names, named) else {
            panic!("{name}: its principal resolves to no SID");
        };
        assert_eq!(resolved == sid, pool, "{name}: {}", resolved.as_str());
    }

    // 5. The sweep's task step, with the test user as the pool.
    let swept = sweep_tasks(&mut sweeper, std::slice::from_ref(&sid));
    eprintln!("5. {swept:?}");
    assert!(swept.deleted >= 1, "{swept:?}");
    assert_eq!(swept.not_deleted, 0, "{swept:?}");
    let left = root_tasks(&mut sweeper);
    assert!(
        !left.iter().any(|task| task.name == theirs),
        "the pool's task is still there"
    );
    assert!(
        left.iter().any(|task| task.name == mine),
        "the lawyer's task is gone"
    );
    eprintln!("6. the clean-up deletes the lawyer's task, then the user");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_this_machines_tasks_all_have_a_principal_that_resolves() {
    if !gate("elevated_this_machines_tasks_all_have_a_principal_that_resolves") {
        return;
    }
    // With an empty pool nothing is deleted, but every principal is still read and resolved:
    // one that isn't would end every real run at 55. Each failure is listed by its kind (the
    // call that failed), never by a task's or an account's name.
    let mut sweeper = WindowsSweeper::new();
    let swept = sweep_tasks(&mut sweeper, &[]);
    eprintln!(
        "{} folders, {} tasks, {} with no user or group, {} unreadable, {} skipped, {} folders \
         unlisted",
        swept.folders,
        swept.tasks,
        swept.no_principal,
        swept.unreadable,
        swept.skipped,
        swept.unlisted
    );
    for (why, count) in &swept.why {
        eprintln!("{count} x {why}");
    }
    assert!(swept.tasks > 0, "no task was read at all");
    assert_eq!(swept.deleted, 0);
    assert!(
        !swept.left(),
        "tasks or folders that can't be read: {:?}",
        swept.why
    );
}
