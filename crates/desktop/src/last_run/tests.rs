//! The last run's mark: a clean quit leaves `closed`, a run that ends without its quit (its lock
//! let go, as the OS does when a process dies) leaves `running`, a second start while one runs
//! touches nothing, and a start that crashes before any quit still counts.

use clauseo_settings::{Files as _, SystemFiles};

use super::{LastRun, MARK_FILE, begin};

fn mark_text(dir: &std::path::Path) -> String {
    String::from_utf8(SystemFiles.read(&dir.join(MARK_FILE)).unwrap().unwrap()).unwrap()
}

#[test]
fn a_first_start_is_clean_and_marks_running() {
    let dir = tempfile::tempdir().unwrap();
    let (last, mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Clean);
    assert!(mark.is_some());
    assert_eq!(mark_text(dir.path()), "running\n");
}

#[test]
fn a_clean_quit_leaves_the_next_start_clean() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles);
    let mark = mark.unwrap();
    mark.close(&SystemFiles);
    assert_eq!(mark_text(dir.path()), "closed\n");
    drop(mark);
    let (last, _mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Clean);
}

#[test]
fn a_run_that_ends_without_its_quit_makes_the_next_start_unclean() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles);
    // A kill: the lock goes with the process, and no quit wrote `closed`.
    drop(mark);
    let (last, mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Unclean);
    // That start marks itself running; a crash during it counts at the start after.
    drop(mark);
    let (last, _mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Unclean);
}

#[test]
fn a_second_start_touches_nothing_and_does_not_trip_the_mark() {
    let dir = tempfile::tempdir().unwrap();
    let (_, first) = begin(dir.path(), &SystemFiles);
    let first = first.unwrap();
    let (second, none) = begin(dir.path(), &SystemFiles);
    assert_eq!(second, LastRun::OtherInstance);
    assert!(none.is_none());
    assert_eq!(mark_text(dir.path()), "running\n");
    // The first quits cleanly: the start after it is clean, the second start notwithstanding.
    first.close(&SystemFiles);
    drop(first);
    let (last, _mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Clean);
}

#[test]
fn a_closed_mark_reads_clean() {
    let dir = tempfile::tempdir().unwrap();
    SystemFiles
        .write_atomic(&dir.path().join(MARK_FILE), b"closed\n")
        .unwrap();
    let (last, _mark) = begin(dir.path(), &SystemFiles);
    assert_eq!(last, LastRun::Clean);
}
