//! A pool task's running instances: stopped after the task is deleted and the pool's BITS jobs
//! cancelled, so none starts in between, matched by its path; any that can't be stopped, or may
//! be its own and can't be read, is 56.

use super::*;

/// A sweep of a pool task `\A\pool` and the lawyer's `\A\other`, with `running` as the running
/// instances.
fn with_running(running: Result<Vec<Result<String, Failed>>, Failed>) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.accounts
        .insert("lawyer".to_owned(), Sid::parse(OWNER).unwrap());
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\A", "other", user("lawyer"));
    fake.running = running;
    fake
}

#[test]
fn a_pool_tasks_running_instances_are_stopped_after_it_is_deleted_and_the_jobs_cancelled() {
    let running = Ok(vec![
        Ok(r"\A\other".to_owned()),
        // The Task Scheduler's paths ignore case.
        Ok(r"\a\POOL".to_owned()),
        Ok(r"\A\pool".to_owned()),
        Ok(r"\pool".to_owned()),
    ]);
    let mut fake = with_running(running);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let at = |line: &str| fake.done.iter().position(|done| done == line).unwrap();
    // Only the pool task's two: not the lawyer's, nor a task of the same name elsewhere.
    let stopped: Vec<&String> = fake
        .done
        .iter()
        .filter(|d| d.starts_with("stop "))
        .collect();
    assert_eq!(stopped, [r"stop \a\POOL", r"stop \A\pool"]);
    // Deleted, then the BITS jobs, then the instances: none can start after its stop.
    assert!(at(r"delete \A\pool") < at("jobs"));
    assert!(at("jobs") < at("running"));
    assert!(at("running") < at(r"stop \A\pool"));
    assert_eq!(fake.deleted(), [r"\A\pool"]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("2 running instances stopped, 0 not"),
        "{line}"
    );
    // No pool task: the running instances aren't even listed.
    let mut quiet = with_running(Ok(vec![Ok(r"\A\other".to_owned())]));
    quiet.folders.get_mut("\\A").unwrap().tasks.remove(0);
    assert_eq!(sweep(&mut quiet), SweepExit::Done);
    assert!(!quiet.done.contains(&"running".to_owned()));
}

#[test]
fn an_instance_not_stopped_or_unread_or_an_unread_list_is_56_and_the_task_still_goes() {
    let refused = || {
        let mut fake = with_running(Ok(vec![Ok(r"\A\pool".to_owned())]));
        fake.stop_refuse = vec![0];
        fake
    };
    let unread = || with_running(Ok(vec![Err("IRunningTask::Path failed (5)".to_owned())]));
    let unlisted = || with_running(Err("ITaskService::GetRunningTasks failed (5)".to_owned()));
    for (case, mut fake) in [
        ("refused", refused()),
        ("unread", unread()),
        ("unlisted", unlisted()),
    ] {
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{case}");
        assert_eq!(fake.deleted(), [r"\A\pool"], "{case}");
        let line = &fake.logged[0].1;
        assert!(
            line.contains("0 running instances stopped, 1 not"),
            "{case}: {line}"
        );
        assert!(!line.contains("A\\pool"), "no task named: {line}");
    }
}

#[test]
fn an_instance_a_trigger_started_before_the_delete_is_still_stopped() {
    // The instance is listed only once the task is gone: one its trigger started at any point
    // before the delete is in that list, and one can't start after it.
    let mut fake = with_running(Ok(vec![Ok(r"\A\pool".to_owned())]));
    fake.task("\\B", "pool2", user("S-1-5-21-1-2-3-1010"));
    fake.running = Ok(vec![Ok(r"\B\pool2".to_owned()), Ok(r"\A\pool".to_owned())]);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let lists = fake.done.iter().filter(|done| *done == "running").count();
    assert_eq!(lists, 1, "listed once, for every pool task");
    let last_delete = fake
        .done
        .iter()
        .rposition(|d| d.starts_with("delete "))
        .unwrap();
    let first_stop = fake
        .done
        .iter()
        .position(|d| d.starts_with("stop "))
        .unwrap();
    assert!(last_delete < first_stop, "{:?}", fake.done);
    assert_eq!(fake.deleted().len(), 2);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("2 running instances stopped, 0 not"),
        "{line}"
    );
}

#[test]
fn a_pool_task_whose_delete_fails_still_has_its_instances_stopped() {
    let mut fake = with_running(Ok(vec![Ok(r"\A\pool".to_owned())]));
    fake.delete_refuse = vec!["pool".to_owned()];
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert!(fake.done.contains(&r"stop \A\pool".to_owned()));
}
