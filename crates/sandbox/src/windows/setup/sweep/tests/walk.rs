//! The walk's failures, each counted without stopping the rest: a folder that can't be listed,
//! a failed delete, the folder cap; and the log line, which names nothing.

use super::*;

#[test]
fn a_folder_that_cant_be_listed_is_56_and_the_rest_are_still_walked() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "hidden-from-us", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\A\\B", "below", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\C", "kept", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\C\\D", "under-c", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\E", "fine", user("S-1-5-21-1-2-3-1010"));
    // \A's tasks can't be listed, but its subfolders can; \C's subfolders can't, but its tasks
    // can; a subfolder of the root can't be read at all.
    fake.folder("\\A").tasks_fail = true;
    fake.folder("\\C").subs_fail = true;
    fake.folder("\\")
        .subs
        .push(Err("ITaskFolderCollection::Item failed".to_owned()));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let mut deleted = fake.deleted();
    deleted.sort_unstable();
    assert_eq!(deleted, ["\\A\\B\\below", "\\C\\kept", "\\E\\fine"]);
    let line = &fake.logged[0].1;
    assert!(line.contains("; 3 folders couldn't be listed"), "{line}");
}

#[test]
fn a_failed_delete_is_56_and_the_task_is_counted() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "stuck", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\", "goes", user("S-1-5-21-1-2-3-1010"));
    fake.delete_refuse = vec!["stuck".to_owned()];
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), ["\\goes"]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 of the pool's tasks deleted, 0 unreadable"),
        "{line}"
    );
    assert!(line.contains("no SID), 1 not deleted"), "{line}");
}

#[test]
fn more_folders_than_the_cap_is_56_and_the_cap_itself_is_not() {
    for (extra, exit) in [(0, SweepExit::Done), (1, SweepExit::TasksLeft)] {
        let mut fake = Fake::new(&[1010]);
        // The root and FOLDERS_CAP - 1 below it, then `extra` more.
        for at in 0..FOLDERS_CAP - 1 + extra {
            fake.folder(&format!("\\F{at}"));
        }
        assert_eq!(sweep(&mut fake), exit, "{extra}");
        let line = &fake.logged[0].1;
        assert!(
            line.contains(&format!("{FOLDERS_CAP} folders and")),
            "{line}"
        );
        assert_eq!(
            line.contains("more than 10000 folders"),
            extra == 1,
            "{line}"
        );
    }
}

#[test]
fn the_log_line_holds_no_sid_task_name_or_account_name() {
    let mut fake = Fake::new(&[1010]);
    fake.accounts.insert("ClauseoSb01".to_owned(), sid(1010));
    fake.task("\\Secret", "MatterTask", user("ClauseoSb01"));
    fake.task("\\Secret", "Stuck", user("S-1-5-21-1-2-3-1010"));
    fake.unread("\\Secret", "Unread");
    fake.task("\\Secret", "Skipped", user(r"CORP\unknown-person"));
    fake.task("\\Secret", "Unresolved", user("gone-person"));
    fake.delete_refuse = vec!["Stuck".to_owned()];
    fake.jobs = Ok(vec![Some(sid(1010)), None]);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let line = &fake.logged[0].1;
    // Only the tasks that keep it at 56 are named, by path; never a task deleted or skipped,
    // a SID or an account's name.
    let named =
        r"; tasks that keep the sweep at 56: \Secret\Stuck, \Secret\Unread, \Secret\Unresolved";
    assert!(line.ends_with(named), "{line}");
    for secret in [
        "S-1-",
        "MatterTask",
        "Skipped",
        "ClauseoSb",
        "CORP",
        "unknown",
        "gone",
    ] {
        assert!(!line.contains(secret), "{secret}: {line}");
    }
    // And the counts.
    assert!(
        line.contains("1 of the pool's tasks deleted, 1 unreadable (0 more skipped"),
        "{line}"
    );
    assert!(line.contains(", 1 unresolved (no SID"), "{line}");
    assert!(line.contains(", 1 skipped (another"), "{line}");
    assert!(line.contains("no SID), 1 not deleted"), "{line}");
    assert!(line.contains("1 BITS jobs cancelled, 1 not"), "{line}");
}
