//! A task swapped under its name while hardening reads or writes it (`harden.rs`, the
//! `TaskCache\Tree` `Id` read before the first read, before the write and after it), and the
//! record's `OwnerUntrusted` the next sweep holds such a task to (`untrusted.rs`).

use super::hardening::{descriptor, loose, loose_dacl};
use super::*;

/// The `Id` every task has until a swap.
const ORIGINAL: &str = "{00000000-0000-0000-0000-000000000001}";
/// The `Id` of the task swapped in.
const SWAPPED: &str = "{00000000-0000-0000-0000-0000000000ff}";

/// The fake's cache `Id`s, and the record's `OwnerUntrusted`.
#[derive(Default)]
pub(super) struct Ids {
    /// Each path's `Id` reads so far.
    reads: BTreeMap<String, usize>,
    /// Swaps: from the given read of the path on (1-based), its `Id` is the one given.
    pub(super) swaps: Vec<(String, usize, String)>,
    /// The record's `OwnerUntrusted`.
    pub(super) record: Vec<String>,
    /// Whether the set-up lock was held at each write of it.
    pub(super) held: Vec<bool>,
    /// Its writes fail.
    pub(super) store_fails: bool,
    /// From the given read of the path on (1-based), its `Id` can't be read.
    pub(super) unread_from: Option<(String, usize)>,
}

/// The `Id` of `fake`'s task at `path`: [`ORIGINAL`], or a swap's from its read on; `None`
/// once the task is gone.
pub(super) fn cache_id(fake: &mut Fake, path: &str) -> Result<Option<String>, Failed> {
    if !fake.on_disk().iter().any(|held| held == path) {
        return Ok(None);
    }
    let reads = fake.ids.reads.entry(path.to_owned()).or_default();
    *reads += 1;
    let read = *reads;
    if let Some((at, from)) = &fake.ids.unread_from
        && at == path
        && read >= *from
    {
        return Err("RegQueryValueExW failed (5)".to_owned());
    }
    let mut swap = fake.ids.swaps.iter().rev();
    let swap = swap.find(|(at, from, _)| at == path && read >= *from);
    Ok(Some(
        swap.map_or(ORIGINAL, |(_, _, id)| id.as_str()).to_owned(),
    ))
}

/// An administrator's loose task `\A\loose` that runs as the lawyer, its `Id` swapped from
/// read `from` on.
fn swapped_at(from: usize) -> Fake {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.ids
        .swaps
        .push(("\\A\\loose".to_owned(), from, SWAPPED.to_owned()));
    fake
}

#[test]
fn an_id_that_changes_before_the_write_means_no_write_and_a_swap_counted() {
    // Read 1 before the descriptor, read 2 just before the write.
    let mut fake = swapped_at(2);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured, []);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 couldn't be, 0 couldn't be read, 1 tasks another task took the name"),
        "{line}"
    );
    let admin = fake.admin_logged.join("\n");
    assert!(
        admin.contains("didn't harden a task that runs as the lawyer, \"\\A\\loose\": another"),
        "{admin}"
    );
    assert_eq!(fake.ids.record, Vec::<String>::new());
}

#[test]
fn an_id_that_changes_across_the_write_ends_the_run_56_and_distrusts_the_owner() {
    // Read 3 after the write.
    let mut fake = swapped_at(3);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured.len(), 1);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 tasks another task took the name"),
        "{line}"
    );
    let admin = fake.admin_logged.join("\n");
    assert!(
        admin.contains("\"\\A\\loose\", but another task may have taken its name"),
        "{admin}"
    );
    // In the 56 list, and recorded with the new `Id`.
    assert!(line.contains("1 tasks keep it at 56"), "{line}");
    let listed = fake.admin_logged.iter().filter(|l| !l.contains("lawyer"));
    assert!(listed.clone().any(|l| l.contains("\\A\\loose")), "{admin}");
    let entry = format!("{SWAPPED}\\A\\loose");
    assert_eq!(fake.ids.record, std::slice::from_ref(&entry));

    // The next sweep: the owner hardening wrote is ignored. Its principal is the lawyer, so
    // only its owner could pass it: 56 again, not hardened again, the entry kept.
    fake.logged.clear();
    fake.admin_logged.clear();
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured.len(), 1);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    assert_eq!(fake.ids.record, [entry]);

    // The swapped-in task names a pool user: deleted, and the entry clears with it.
    let tasks = &mut fake.folders.get_mut("\\A").unwrap().tasks;
    tasks.retain(|task| task.name != "loose");
    fake.task("\\A", "loose", user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\A\\loose"]);
    assert_eq!(fake.ids.record, Vec::<String>::new());
}

#[test]
fn an_entry_whose_id_no_longer_matches_clears_and_the_task_is_judged_as_usual() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.ids.record = vec![format!("{SWAPPED}\\A\\loose")];
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured.len(), 1, "hardened as usual");
    assert_eq!(fake.ids.record, Vec::<String>::new());
}

#[test]
fn the_normal_path_reads_the_id_three_times_and_a_clean_write_removes_its_entry() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured.len(), 1);
    assert_eq!(fake.ids.reads.get("\\A\\loose"), Some(&3));
    // Written ahead under the lock, then taken off.
    assert_eq!(fake.ids.held, [true, true]);
    assert_eq!(fake.ids.record, Vec::<String>::new());
    assert!(!fake.logged[0].1.contains("took the name"));
}

#[test]
fn an_entry_that_cant_be_written_ahead_means_no_write() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.ids.store_fails = true;
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured, []);
    let line = &fake.logged[0].1;
    assert!(line.contains("write removed, 1 couldn't be"), "{line}");
    let admin = fake.admin_logged.join("\n");
    assert!(
        admin.contains("its OwnerUntrusted entry couldn't be written"),
        "{admin}"
    );
}

#[test]
fn an_id_unread_after_the_write_keeps_an_any_id_entry_and_the_owner_untrusted() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.ids.unread_from = Some(("\\A\\loose".to_owned(), 3));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured.len(), 1);
    let entry = "{*}\\A\\loose".to_owned();
    assert_eq!(fake.ids.record, std::slice::from_ref(&entry));
    // The next sweep reads an `Id` again: any matches, so the owner is ignored and the
    // lawyer's task is 56, not hardened again.
    fake.ids.unread_from = None;
    fake.logged.clear();
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured.len(), 1);
    assert_eq!(fake.ids.record, [entry]);
    // Gone: the entry clears.
    let tasks = &mut fake.folders.get_mut("\\A").unwrap().tasks;
    tasks.retain(|task| task.name != "loose");
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.ids.record, Vec::<String>::new());
}
