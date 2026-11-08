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
}

/// The `Id` of `fake`'s task at `path`: [`ORIGINAL`], or a swap's from its read on; `None`
/// once the task is gone.
pub(super) fn cache_id(fake: &mut Fake, path: &str) -> Option<String> {
    if !fake.on_disk().iter().any(|held| held == path) {
        return None;
    }
    let reads = fake.ids.reads.entry(path.to_owned()).or_default();
    *reads += 1;
    let read = *reads;
    let mut swap = fake.ids.swaps.iter().rev();
    let swap = swap.find(|(at, from, _)| at == path && read >= *from);
    Some(swap.map_or(ORIGINAL, |(_, _, id)| id.as_str()).to_owned())
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
fn the_normal_path_reads_the_id_three_times_and_records_nothing() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured.len(), 1);
    assert_eq!(fake.ids.reads.get("\\A\\loose"), Some(&3));
    assert_eq!(fake.ids.record, Vec::<String>::new());
    assert!(!fake.logged[0].1.contains("took the name"));
}
