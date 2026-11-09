//! Orphan cache keys: a `Tree` key Windows left after a feature update or an uninstall, holding
//! `SD`, `Id` and `Index` and nothing more, with no task past it, no task file and an owner that
//! isn't a pool SID, is counted apart and isn't 56. A task hidden by stripping its `SD`
//! (Tarrask), or owned by a pool user, or anything that can't be read, still is. Nothing is
//! deleted.

use super::task_file::file;
use super::*;
use crate::windows::acl_bytes::owner_sid;
use crate::windows::setup::record_values::SWEEP_SEQUENCE;

/// The orphan keys of a Windows VM's export (`TaskCache\Tree\…`), each its `SD`, `Id` and
/// `Index` (0) and no task past it.
const ORPHANS: [&str; 26] = [
    r"\Microsoft\OneCore\DirectX\DirectXDatabaseUpdater",
    r"\Microsoft\Windows\Application Experience\AitAgent",
    r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
    r"\Microsoft\Windows\Application Experience\ProgramDataUpdater",
    r"\Microsoft\Windows\Customer Experience Improvement Program\KernelCeipTask",
    r"\Microsoft\Windows\MemoryDiagnostic\CorruptionDetector",
    r"\Microsoft\Windows\MemoryDiagnostic\DecompressionFailureDetector",
    r"\Microsoft\Windows\MemoryDiagnostic\MemUsageTask",
    r"\Microsoft\Windows\NetTrace\GatherNetworkInfo",
    r"\Microsoft\Windows\PLA\System\ConvertLogEntries",
    r"\Microsoft\Windows\Shell\CrawlStartPages",
    r"\Microsoft\Windows\Software Inventory Logging\Daily Collector",
    r"\Microsoft\Windows\TaskScheduler\Idle Maintenance",
    r"\Microsoft\Windows\TaskScheduler\Maintenance Configurator",
    r"\Microsoft\Windows\TaskScheduler\Manual Maintenance",
    r"\Microsoft\Windows\TaskScheduler\Regular Maintenance",
    r"\Microsoft\Windows\Windows Activation Technologies\ValidationTask",
    r"\Microsoft\Windows\Windows Activation Technologies\ValidationTaskDeadline",
    r"\Microsoft\Windows\Windows Subsystem For Linux\AptPackageIndexUpdate",
    r"\Microsoft\Windows\WindowsUpdate\Automatic App Update",
    r"\Microsoft\Windows\WindowsUpdate\sihboot",
    r"\Microsoft\Windows\WS\Badge Update",
    r"\Microsoft\Windows\WS\License Validation",
    r"\Microsoft\Windows\WS\Sync Licenses",
    r"\Microsoft\Windows\WS\WSRefreshBannedAppsListTask",
    r"\Microsoft\Windows\WS\WSTask",
];

/// The `SD` of `…\TaskScheduler\Maintenance Configurator` in the export: owned by
/// Administrators.
const ADMINS_SD: [u8; 116] = [
    0x01, 0x00, 0x04, 0x9c, 0x48, 0x00, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x02, 0x00, 0x34, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00,
    0xa9, 0x00, 0x12, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x20, 0x02, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00, 0x00, 0x01, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x15, 0x00, 0x00, 0x00, 0x27, 0x37, 0x5c, 0x6e, 0xe9, 0x05, 0xa7, 0xcc, 0x5f, 0xf0, 0x0e, 0xd3,
    0x01, 0x02, 0x00, 0x00,
];

/// The `SD` of `…\PLA\System\ConvertLogEntries` in the export: owned by SYSTEM.
const SYSTEM_SD: [u8; 112] = [
    0x01, 0x00, 0x04, 0x9c, 0x48, 0x00, 0x00, 0x00, 0x54, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x02, 0x00, 0x34, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00,
    0xff, 0x01, 0x1f, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x20, 0x02, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x12, 0x00, 0x00, 0x00, 0x01, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x15, 0x00, 0x00, 0x00,
    0x27, 0x37, 0x5c, 0x6e, 0xe9, 0x05, 0xa7, 0xcc, 0x5f, 0xf0, 0x0e, 0xd3, 0x01, 0x02, 0x00, 0x00,
];

/// Why an `SD` that is gone (Tarrask deletes it) can't be read.
const NO_SD: &str = "a task's descriptor in the Task Scheduler's cache couldn't be read (2)";

/// A fake of pool user 1010 with its task `\A\pool` listed, and `path` a key only the cache
/// holds, owned by `owner`, with a task past it or not as `entries` says.
fn cache_only(path: &str, owner: Result<Sid, Failed>, entries: Result<bool, Failed>) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    fake.cache_only.push(path.to_owned());
    fake.cache_owners.insert(path.to_owned(), owner);
    fake.cache_entries.insert(path.to_owned(), entries);
    fake
}

/// Asserts `fake` swept to 56 with one key not listed, none an orphan, and only `\A\pool`
/// deleted.
fn hidden_56(mut fake: Fake, case: &str) {
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{case}");
    assert_eq!(fake.deleted(), [r"\A\pool"], "{case}");
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 not listed (in a task file or the cache only), 0 orphan"),
        "{line}"
    );
}

#[test]
fn the_orphan_keys_windows_leaves_are_counted_and_the_run_is_clean() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    for path in ORPHANS {
        let sd: &[u8] = if path.ends_with("ConvertLogEntries") {
            &SYSTEM_SD
        } else {
            &ADMINS_SD
        };
        fake.cache_only.push(path.to_owned());
        fake.cache_owners
            .insert(path.to_owned(), Ok(owner_sid(sd).unwrap()));
        fake.cache_entries.insert(path.to_owned(), Ok(false));
    }
    let system = &fake.cache_owners[ORPHANS[9]];
    assert_eq!(system, &Ok(Sid::parse("S-1-5-18").unwrap()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), [r"\A\pool"], "no orphan is deleted");
    assert_eq!(
        fake.values.get(SWEEP_SEQUENCE),
        Some(&1),
        "the sequence advances"
    );
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert!(
        line.contains(
            "0 not listed (in a task file or the cache only), 26 orphan cache keys (no task, not \
             the pool's)"
        ),
        "{line}"
    );
    assert!(!line.contains("Microsoft"), "no key named: {line}");
}

#[test]
fn a_task_hidden_by_stripping_or_owning_its_sd_is_56() {
    // Tarrask: the `SD` deleted, or a pool user's, while `Tasks\{Id}` still holds the task.
    let path = r"\Microsoft\Windows\Updates\hidden";
    for owner in [Err(NO_SD.to_owned()), Ok(sid(1010))] {
        hidden_56(
            cache_only(path, owner.clone(), Ok(true)),
            &format!("{owner:?}"),
        );
        // Its task file kept, naming the pool user: the file listing has it too.
        let mut on_file = cache_only(path, owner.clone(), Ok(true));
        on_file.cache_only.clear();
        on_file.hidden.push(path.to_owned());
        let pool = file("<UserId>S-1-5-21-1-2-3-1010</UserId>");
        on_file
            .files
            .insert(r"Microsoft\Windows\Updates\hidden".to_owned(), Ok(pool));
        hidden_56(on_file, &format!("on file, {owner:?}"));
    }
}

#[test]
fn a_pool_owned_key_with_no_task_past_it_is_56() {
    let fake = cache_only(r"\Microsoft\Windows\WS\WSTask", Ok(sid(1010)), Ok(false));
    hidden_56(fake, "pool owner");
}

#[test]
fn a_key_with_a_task_past_it_or_a_task_file_is_held_to_the_file_rule() {
    let path = r"\Microsoft\Windows\WS\WSTask";
    let admins = || Ok(Sid::parse("S-1-5-32-544").unwrap());
    // `Tasks\{Id}` there and no task file: 56, as before.
    hidden_56(cache_only(path, admins(), Ok(true)), "a task past it");
    // A task file there and no task past the key: the file is read, and a missing one is 56.
    let mut on_file = cache_only(path, admins(), Ok(false));
    on_file.cache_only.clear();
    on_file.hidden.push(path.to_owned());
    hidden_56(on_file, "on file");
    // A task past the key whose trusted file names SYSTEM: skipped by its file, as before.
    let mut skipped = cache_only(path, admins(), Ok(true));
    skipped.cache_only.clear();
    skipped.hidden.push(path.to_owned());
    let system = file("<UserId>S-1-5-18</UserId>");
    skipped
        .files
        .insert(r"Microsoft\Windows\WS\WSTask".to_owned(), Ok(system));
    assert_eq!(sweep(&mut skipped), SweepExit::Done);
    let line = &skipped.logged[0].1;
    assert!(line.contains("1 more skipped"), "{line}");
    assert!(line.contains("0 orphan cache keys"), "{line}");
}

#[test]
fn a_key_whose_sd_or_entries_cant_be_read_is_56() {
    let path = r"\Microsoft\Windows\WS\WSTask";
    let admins = || Ok(Sid::parse("S-1-5-32-544").unwrap());
    hidden_56(cache_only(path, Err(NO_SD.to_owned()), Ok(false)), "no SD");
    let unread = "a task's Id in the Task Scheduler's cache isn't a GUID".to_owned();
    hidden_56(cache_only(path, admins(), Err(unread)), "no Id");
    // With the file listing unread, no key is shown to have no task file.
    let mut files = cache_only(path, admins(), Ok(false));
    files.listings_fail[0] = true;
    assert_eq!(sweep(&mut files), SweepExit::TasksLeft);
    let line = &files.logged[0].1;
    assert!(line.contains("0 orphan cache keys"), "{line}");
}
