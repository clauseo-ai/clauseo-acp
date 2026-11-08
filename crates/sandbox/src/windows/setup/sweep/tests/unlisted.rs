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
