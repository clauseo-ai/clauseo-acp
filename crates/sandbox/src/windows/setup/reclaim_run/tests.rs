//! `--reclaim` once the record is read, on Windows CI as the runner's own user: a pool another
//! user set up exits `OtherOwner`, an unfinished removal and a missing or unreadable record
//! exit `Failed`, and a recorded data folder the owner doesn't own exits `DataDirNotOwned`; in
//! each, nothing is walked and no result file is written. And the result file's counts.

use super::*;
use crate::test_support::TempDir;
use crate::windows::setup::result_file::path as result_path;

const GROUP: &str = "S-1-5-21-1-2-3-1010";
const POOL: &str = "S-1-5-21-1-2-3-1101";
const SOMEONE_ELSE: &str = "S-1-5-21-1-2-3-1500";

fn record(owner: &Sid, data_dir: PathBuf, removing: bool) -> MachineRecord {
    MachineRecord {
        owner: owner.clone(),
        group: Sid::parse(GROUP).unwrap(),
        pool: vec![Sid::parse(POOL).unwrap()],
        data_dir,
        removing,
    }
}

fn data_dir(test: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(test);
    let canonical = dunce::canonicalize(dir.path()).unwrap();
    (dir, canonical)
}

#[test]
fn a_pool_another_user_set_up_exits_other_owner_and_touches_nothing() {
    let owner = Owner::of_this_process().unwrap();
    let (_dir, data) = data_dir("reclaim-other-owner");
    let other = Sid::parse(SOMEONE_ELSE).unwrap();

    let exit = run_with(&owner, Ok(Some(record(&other, data.clone(), false))));

    assert_eq!(exit, SetupExit::OtherOwner);
    assert!(!result_path(&data).exists());
}

#[test]
fn an_unfinished_removal_exits_failed_and_touches_nothing() {
    let owner = Owner::of_this_process().unwrap();
    let (_dir, data) = data_dir("reclaim-removing");

    let exit = run_with(&owner, Ok(Some(record(owner.sid(), data.clone(), true))));

    assert_eq!(exit, SetupExit::Failed);
    assert!(!result_path(&data).exists());
}

#[test]
fn a_recorded_data_folder_the_owner_doesnt_own_exits_data_dir_not_owned() {
    let owner = Owner::of_this_process().unwrap();
    // `C:\Windows`: TrustedInstaller's.
    let windows = PathBuf::from(std::env::var_os("SystemRoot").unwrap());

    let exit = run_with(
        &owner,
        Ok(Some(record(owner.sid(), windows.clone(), false))),
    );

    assert_eq!(exit, SetupExit::DataDirNotOwned);
    assert!(!result_path(&windows).exists());
}

#[test]
fn a_missing_or_unreadable_record_exits_failed() {
    let owner = Owner::of_this_process().unwrap();
    assert_eq!(run_with(&owner, Ok(None)), SetupExit::Failed);
    assert_eq!(run_with(&owner, Err(())), SetupExit::Failed);
}

/// Nothing owed: done, nothing taken, and the result file written as the owner.
#[test]
fn nothing_owed_is_done_and_says_none_was_taken() {
    let owner = Owner::of_this_process().unwrap();
    let (_dir, data) = data_dir("reclaim-nothing");

    let exit = run_with(&owner, Ok(Some(record(owner.sid(), data.clone(), false))));

    assert_eq!(exit, SetupExit::Done);
    let written = result_file::read(&data).unwrap().unwrap();
    assert_eq!((written.exit, written.taken), (0, Some(0)));
}

#[test]
fn the_result_counts_only_what_the_walks_reached() {
    let partial = result_of(Reclaimed {
        visited: 9,
        taken: 2,
        failed: 1,
        roots_refused: 1,
        ..Reclaimed::default()
    });
    assert_eq!((partial.exit, partial.taken), (3, Some(2)));
    let cause = partial.cause.unwrap();
    assert!(
        cause.contains("1 items couldn't") && cause.contains("1 recorded items"),
        "{cause}"
    );
    assert!(!cause.contains('\\') && !cause.contains("S-1-"), "{cause}");

    let done = result_of(Reclaimed {
        taken: 4,
        left: 2,
        ..Reclaimed::default()
    });
    assert_eq!((done.exit, done.taken), (0, Some(4)));
    assert!(done.cause.unwrap().contains("2 items no sandbox user owns"));
}
