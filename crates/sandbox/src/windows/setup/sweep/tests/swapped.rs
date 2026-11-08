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
    /// At the first swap's first read, the task at its path becomes one with this principal,
    /// or, with [`Ids::swap_hides`], is gone from its folder's list.
    pub(super) swap_to: Option<Principal>,
    /// The first swap takes the task off its folder's list.
    pub(super) swap_hides: bool,
    /// `Sweeper::folder_at` fails.
    pub(super) folder_fails: bool,
}

/// The folder at `path` in `fake`, unless [`Ids::folder_fails`].
pub(super) fn folder_at(fake: &mut Fake, path: &str) -> Result<String, Failed> {
    if fake.ids.folder_fails || !fake.folders.contains_key(path) {
        return Err("ITaskService::GetFolder failed (0x80070002)".to_owned());
    }
    Ok(path.to_owned())
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
    let id = swap.map_or(ORIGINAL, |(_, _, id)| id.as_str()).to_owned();
    let first = fake.ids.swaps.first();
    if first.is_some_and(|(at, from, _)| at == path && read == *from)
        && (fake.ids.swap_to.is_some() || fake.ids.swap_hides)
    {
        let to = fake.ids.swap_to.take();
        fake.ids.swap_hides = false;
        let (folder, name) = path.rsplit_once('\\').unwrap();
        let folder = if folder.is_empty() { "\\" } else { folder };
        fake.folders
            .get_mut(folder)
            .unwrap()
            .tasks
            .retain(|task| task.name != name);
        if let Some(principal) = to {
            fake.task(folder, name, principal);
        }
    }
    Ok(Some(id))
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
fn an_id_that_changes_before_the_write_means_no_write_a_swap_counted_and_56() {
    // Read 1 by the walk, read 2 before the descriptor, read 3 just before the write.
    let mut fake = swapped_at(3);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    // The task now under the name is judged again: the same principal, an administrator's
    // owner, so it is hardened, and that one write is the only one.
    assert_eq!(fake.secured.len(), 1);
    let line = &fake.logged[0].1;
    assert!(line.contains("1 tasks keep it at 56"), "{line}");
    assert!(
        line.contains("1 tasks another task took the name of after they were read were judged"),
        "{line}"
    );
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
    // Read 4 after the write.
    let mut fake = swapped_at(4);
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
fn a_swap_before_the_write_to_a_pool_task_gets_it_deleted_and_the_run_56() {
    let mut fake = swapped_at(3);
    fake.ids.swap_to = Some(user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured, []);
    assert_eq!(fake.deleted(), ["\\A\\loose"]);
    let line = &fake.logged[0].1;
    assert!(line.contains("1 of the pool's tasks deleted"), "{line}");
    assert!(line.contains("1 tasks keep it at 56"), "{line}");
}

#[test]
fn the_normal_path_reads_the_id_on_the_walk_around_the_write_and_at_the_end() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured.len(), 1);
    // Once by the walk, three times by hardening, once by the end's check (`replaced.rs`).
    assert_eq!(fake.ids.reads.get("\\A\\loose"), Some(&5));
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
    fake.ids.unread_from = Some(("\\A\\loose".to_owned(), 4));
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

/// A fake whose record holds a pending entry for `\A\loose` with [`ORIGINAL`], as a crash
/// between the write-ahead and its removal leaves it.
fn after_a_crash() -> Fake {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.ids.record = vec![format!("P{ORIGINAL}\\A\\loose")];
    fake
}

#[test]
fn a_pending_entry_whose_id_is_unchanged_clears_and_the_task_is_judged_on_its_owner() {
    let mut fake = after_a_crash();
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    // Judged as usual: hardened, its owner trusted.
    assert_eq!(fake.secured.len(), 1);
    assert_eq!(fake.ids.record, Vec::<String>::new());
}

#[test]
fn a_pending_entry_whose_id_changed_settles_untrusted_and_is_56() {
    let mut fake = after_a_crash();
    fake.ids
        .swaps
        .push(("\\A\\loose".to_owned(), 1, SWAPPED.to_owned()));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured, []);
    assert_eq!(fake.ids.record, [format!("{SWAPPED}\\A\\loose")]);
}

#[test]
fn a_pending_entry_whose_id_cant_be_read_settles_as_any_id() {
    let mut fake = after_a_crash();
    fake.ids.unread_from = Some(("\\A\\loose".to_owned(), 1));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.secured, []);
    assert_eq!(fake.ids.record, ["{*}\\A\\loose".to_owned()]);
}

#[test]
fn a_pending_entry_whose_task_is_gone_clears() {
    let mut fake = after_a_crash();
    let tasks = &mut fake.folders.get_mut("\\A").unwrap().tasks;
    tasks.retain(|task| task.name != "loose");
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.ids.record, Vec::<String>::new());
}
