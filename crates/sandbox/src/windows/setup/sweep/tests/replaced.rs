//! A task replaced under its name after the walk judged it (`replaced.rs`): the walk's paths
//! are held by path and `TaskCache\Tree` `Id` at the end, and one whose `Id` changed is judged
//! again, or 56 when it can't be.

use super::*;

/// The `Id` of the task swapped in.
const SWAPPED: &str = "{00000000-0000-0000-0000-0000000000ee}";

/// SYSTEM's task `\A\t`, its `Id` swapped at the end's check (read 2; read 1 is the walk's).
fn replaced_at_the_end() -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "t", user("S-1-5-18"));
    fake.ids
        .swaps
        .push(("\\A\\t".to_owned(), 2, SWAPPED.to_owned()));
    fake
}

/// The admin-only log's lines that name the tasks keeping the run at 56.
fn named_56(fake: &Fake) -> String {
    fake.admin_logged.join("\n")
}

#[test]
fn a_task_replaced_after_the_walk_by_a_pool_task_is_judged_again_and_deleted() {
    let mut fake = replaced_at_the_end();
    fake.ids.swap_to = Some(user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\A\\t"]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains(
            "1 tasks another task took the name of after they were read were judged \
             again, 0 couldn't be"
        ),
        "{line}"
    );
}

#[test]
fn a_task_replaced_after_the_walk_by_one_not_the_pools_is_judged_again_and_left() {
    let mut fake = replaced_at_the_end();
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(!swept.left(), "{swept:?}");
    assert_eq!(swept.rejudged, ["\\A\\t"]);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
}

#[test]
fn a_replaced_task_whose_folder_cant_be_read_again_is_56() {
    let mut fake = replaced_at_the_end();
    fake.ids.folder_fails = true;
    fake.ids.swap_to = Some(user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let line = &fake.logged[0].1;
    assert!(line.contains("judged again, 1 couldn't be"), "{line}");
    assert!(named_56(&fake).contains("\\A\\t"), "{}", named_56(&fake));
}

#[test]
fn an_id_that_cant_be_read_at_the_end_is_56() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "t", user("S-1-5-18"));
    fake.ids.unread_from = Some(("\\A\\t".to_owned(), 2));
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(swept.left(), "{swept:?}");
    assert_eq!(swept.unjudged, 1);
    assert_eq!(swept.rejudged, Vec::<String>::new());
    assert_eq!(swept.left_paths, ["\\A\\t"]);
}

#[test]
fn an_id_that_cant_be_read_by_the_walk_is_56() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "t", user("S-1-5-18"));
    fake.ids.unread_from = Some(("\\A\\t".to_owned(), 1));
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(swept.left(), "{swept:?}");
    assert_eq!(swept.unjudged, 1);
}

#[test]
fn a_replacement_the_listing_hides_but_the_cache_holds_is_56() {
    let mut fake = replaced_at_the_end();
    fake.ids.swap_hides = true;
    fake.present_again = Ok(true);
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(swept.left(), "{swept:?}");
    assert_eq!(swept.unjudged, 1);
    assert_eq!(swept.left_paths, ["\\A\\t"]);
}

#[test]
fn a_replaced_task_gone_by_its_listing_and_its_cache_key_is_no_task() {
    let mut fake = replaced_at_the_end();
    fake.ids.swap_hides = true;
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(!swept.left(), "{swept:?}");
    assert_eq!(swept.rejudged, ["\\A\\t"]);
}

#[test]
fn an_unchanged_id_judges_nothing_again() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "t", user("S-1-5-18"));
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    let swept = sweep_tasks(&mut fake, &[sid(1010)], None);
    assert!(!swept.left(), "{swept:?}");
    assert_eq!(swept.rejudged, Vec::<String>::new());
    assert_eq!(swept.unjudged, 0);
    assert_eq!(fake.deleted(), ["\\A\\pool"]);
    // Read by the walk and at the end; the pool's task, deleted, reads as gone.
    assert_eq!(swept.walked_ids.len(), 2);
    assert_eq!(swept.tasks, 2);
}
