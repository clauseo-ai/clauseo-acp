//! The cross-check: a task the task files or the Task Scheduler's cache hold but the listing
//! didn't give (its descriptor shuts SYSTEM out, say) is 56, unless its trusted task file names
//! a principal that isn't the pool's. Nothing is deleted through a file or the registry.

use super::task_file::file;
use super::*;

/// A fake of pool user 1010, the lawyer's `\A\task` listed, and `path` hidden from the listing,
/// with its task file, when given, as its key below `Tasks` and its bytes.
fn hiding(path: &str, file_at: Option<(&str, Vec<u8>)>) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.accounts
        .insert("lawyer".to_owned(), Sid::parse(OWNER).unwrap());
    fake.task("\\A", "task", user("lawyer"));
    fake.hidden.push(path.to_owned());
    if let Some((key, bytes)) = file_at {
        fake.files.insert(key.to_owned(), Ok(bytes));
    }
    fake
}

#[test]
fn a_task_the_listing_hides_is_56_and_left() {
    for path in [r"\A\hidden", r"\hidden"] {
        // No task file to read: the task may be the pool's.
        let mut fake = hiding(path, None);
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{path}");
        assert!(fake.deleted().is_empty(), "never deleted through a file");
        let line = &fake.logged[0].1;
        assert!(line.contains("1 not listed (in a task file"), "{line}");
        assert!(!line.contains("hidden"), "no task named: {line}");
    }
    // A file naming a pool user, or owned by one, is no proof either.
    let pool = file("<UserId>S-1-5-21-1-2-3-1010</UserId>");
    let mut named = hiding(r"\A\hidden", Some((r"A\hidden", pool)));
    assert_eq!(sweep(&mut named), SweepExit::TasksLeft);
    let lawyers = file("<UserId>lawyer</UserId>");
    let mut owned = hiding(r"\A\hidden", Some((r"A\hidden", lawyers)));
    let mut security = clean();
    security.owner = sid(1010);
    owned.securities.insert(r"A\hidden".to_owned(), security);
    assert_eq!(sweep(&mut owned), SweepExit::TasksLeft);
}

#[test]
fn a_hidden_task_whose_trusted_file_names_another_principal_is_skipped() {
    let system = file("<UserId>S-1-5-18</UserId>");
    let mut fake = hiding(r"\A\hidden", Some((r"A\hidden", system)));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let line = &fake.logged[0].1;
    assert!(line.contains("0 not listed"), "{line}");
    assert!(line.contains("1 more skipped"), "{line}");
}

#[test]
fn a_hidden_task_owned_by_a_pool_user_or_whose_cache_descriptor_cant_be_read_is_56() {
    let system = || file("<UserId>S-1-5-18</UserId>");
    let missing = "a task's descriptor in the Task Scheduler's cache couldn't be read (2)";
    for owner in [Ok(sid(1010)), Err(missing.to_owned())] {
        // A trusted file naming SYSTEM, which a pool owner may have rewritten the task past.
        let mut fake = hiding(r"\A\hidden", Some((r"A\hidden", system())));
        fake.cache_owners
            .insert(r"\A\hidden".to_owned(), owner.clone());
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{owner:?}");
        assert!(
            fake.deleted().is_empty(),
            "never deleted through the registry"
        );
        let line = &fake.logged[0].1;
        assert!(line.contains("1 not listed"), "{line}");
        assert!(!line.contains("1 more skipped"), "{line}");
    }
    // Another owner, with that file: skipped.
    let mut other = hiding(r"\A\hidden", Some((r"A\hidden", system())));
    other
        .cache_owners
        .insert(r"\A\hidden".to_owned(), Ok(sid(1001)));
    assert_eq!(sweep(&mut other), SweepExit::Done);
}

#[test]
fn a_listing_that_cant_be_read_is_56() {
    let mut files = hiding(r"\A\task", None);
    files.listings_fail[0] = true;
    assert_eq!(sweep(&mut files), SweepExit::TasksLeft);
    let mut cache = hiding(r"\A\task", None);
    cache.listings_fail[1] = true;
    assert_eq!(sweep(&mut cache), SweepExit::TasksLeft);
}

#[test]
fn a_task_is_listed_under_another_case_but_not_under_the_kelvin_sign() {
    // The same task as Windows compares names: listed.
    let mut case = hiding(r"\a\TASK", None);
    assert_eq!(sweep(&mut case), SweepExit::Done);
    // U+212A lowercases to `k`, but `\A\tas\u{212A}` is another task: hidden.
    let mut kelvin = hiding("\\A\\tas\u{212A}", None);
    assert_eq!(sweep(&mut kelvin), SweepExit::TasksLeft);
}

#[test]
fn every_listed_task_on_file_and_in_the_cache_passes() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "root", user("S-1-5-18"));
    fake.task("\\A\\B", "deep", user("S-1-5-18"));
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), [r"\A\pool"]);
}

/// The sweep of `fake`, its exit and its log line, which must name no path or SID.
fn swept_line(fake: &mut Fake) -> (SweepExit, String) {
    let exit = sweep(fake);
    let line = fake.logged[0].1.clone();
    assert!(
        !line.contains("stray") && !line.contains("stale"),
        "no path: {line}"
    );
    assert!(!line.contains("S-1-"), "no SID: {line}");
    (exit, line)
}

#[test]
fn a_stray_task_file_is_ignored_unless_a_pool_user_owns_it_or_its_owner_cant_be_read() {
    let other = || Ok(Sid::parse(OWNER).unwrap());
    for owner in [
        None,
        Some(Ok(Sid::parse("S-1-5-18").unwrap())),
        Some(other()),
    ] {
        let mut fake = hiding(r"\A\task", None);
        fake.stray.push(r"\A\stray".to_owned());
        if let Some(owner) = owner.clone() {
            fake.file_owners.insert(r"A\stray".to_owned(), owner);
        }
        let (exit, line) = swept_line(&mut fake);
        assert_eq!(exit, SweepExit::Done, "{owner:?}: {line}");
        let kind = "1 task files with no cache key (1 ignored, not a pool user's)";
        assert!(line.contains(kind), "{line}");
        assert!(line.contains("0 not listed"), "{line}");
    }
    let unread = Err("the task file's owner couldn't be read (5)".to_owned());
    for owner in [Ok(sid(1010)), unread] {
        let mut fake = hiding(r"\A\task", None);
        fake.stray.push(r"\A\stray".to_owned());
        fake.file_owners
            .insert(r"A\stray".to_owned(), owner.clone());
        let (exit, line) = swept_line(&mut fake);
        assert_eq!(exit, SweepExit::TasksLeft, "{owner:?}");
        assert!(line.contains("1 not listed"), "{line}");
        let kind = "1 task files with no cache key (0 ignored";
        assert!(line.contains(kind), "{line}");
        assert!(fake.deleted().is_empty(), "never deleted through a file");
    }
}

#[test]
fn a_stale_cache_key_is_ignored_unless_its_descriptor_names_a_pool_owner() {
    let missing = "a task's descriptor in the Task Scheduler's cache couldn't be read (2)";
    for owner in [None, Some(Err(missing.to_owned())), Some(Ok(sid(1001)))] {
        let mut fake = hiding(r"\A\task", None);
        fake.stale.push(r"\stale".to_owned());
        if let Some(owner) = owner.clone() {
            fake.cache_owners.insert(r"\stale".to_owned(), owner);
        }
        let (exit, line) = swept_line(&mut fake);
        assert_eq!(exit, SweepExit::Done, "{owner:?}: {line}");
        let kind = "1 cache keys with no task file (1 ignored";
        assert!(line.contains(kind), "{line}");
    }
    let mut pool = hiding(r"\A\task", None);
    pool.stale.push(r"\stale".to_owned());
    pool.cache_owners
        .insert(r"\stale".to_owned(), Ok(sid(1010)));
    let (exit, line) = swept_line(&mut pool);
    assert_eq!(exit, SweepExit::TasksLeft);
    assert!(line.contains("1 not listed"), "{line}");
    assert!(
        line.contains("1 cache keys with no task file (0 ignored"),
        "{line}"
    );
    assert!(
        pool.deleted().is_empty(),
        "never deleted through the registry"
    );
}

#[test]
fn a_task_made_or_deleted_mid_run_or_a_key_gone_mid_listing_is_read_again_not_56() {
    // Made once the walk was done: the second walk lists it.
    let mut late = Fake::new(&[1010]);
    late.late.push((r"\A".to_owned(), "late".to_owned()));
    let (exit, line) = swept_line(&mut late);
    assert_eq!(exit, SweepExit::Done, "{line}");
    assert!(line.contains("were read again: 0 tasks"), "{line}");
    // Deleted between the two reads, a pool user's: its file alone, then nothing.
    let mut gone = Fake::new(&[1010]);
    gone.once.push(r"\A\gone".to_owned());
    gone.file_owners.insert(r"A\gone".to_owned(), Ok(sid(1010)));
    let (exit, line) = swept_line(&mut gone);
    assert_eq!(exit, SweepExit::Done, "{line}");
    assert!(line.contains("0 task files with no cache key"), "{line}");
    // A key gone mid-listing, in the first read alone or in both: left out, never 56.
    for (vanish, counted) in [(1, "0 folders or keys gone"), (2, "1 folders or keys gone")] {
        let mut fake = Fake::new(&[1010]);
        fake.task(r"\A", "task", user("S-1-5-18"));
        fake.vanish = vanish;
        let (exit, line) = swept_line(&mut fake);
        assert_eq!(exit, SweepExit::Done, "{line}");
        assert!(line.contains(counted), "{line}");
    }
    // Listings that agree aren't read again.
    let mut agree = Fake::new(&[1010]);
    let (_, line) = swept_line(&mut agree);
    assert!(!line.contains("read again"), "{line}");
    assert_eq!(agree.listings, 1);
}

#[test]
fn a_pool_owned_hidden_task_is_still_56_after_the_second_read() {
    let system = file("<UserId>S-1-5-18</UserId>");
    let mut fake = hiding(r"\A\hidden", Some((r"A\hidden", system)));
    fake.cache_owners
        .insert(r"\A\hidden".to_owned(), Ok(sid(1010)));
    let (exit, line) = swept_line(&mut fake);
    assert_eq!(exit, SweepExit::TasksLeft);
    assert_eq!(fake.listings, 2, "read again once");
    assert!(line.contains("1 not listed"), "{line}");
    let kind = "were read again: 1 tasks in a task file and the cache";
    assert!(line.contains(kind), "{line}");
    let swept = sweep_tasks(&mut fake, &[sid(1010)]);
    assert_eq!(swept.hidden_paths, [r"\A\hidden"]);
}
