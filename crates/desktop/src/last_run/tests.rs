//! The last run's mark: a clean quit leaves `closed` and lets go of the lock, a run that ends
//! without its quit (its lock let go, as the OS does when a process dies) leaves `running`, a
//! second start while one runs touches nothing, a start that waits out a lock being let go is a
//! normal start, a start that crashes before any quit still counts, and a refused quit write is
//! tried again.

use std::cell::Cell;
use std::io;
use std::path::Path;
use std::time::Duration;

use clauseo_settings::{Files, SystemFiles};

use super::{LOCK_TRIES, LastRun, MARK_FILE, begin};

fn mark_text(dir: &Path) -> String {
    String::from_utf8(SystemFiles.read(&dir.join(MARK_FILE)).unwrap().unwrap()).unwrap()
}

/// A pause that waits for nothing.
fn none(_: Duration) {}

#[test]
fn a_first_start_is_clean_and_marks_running() {
    let dir = tempfile::tempdir().unwrap();
    let (last, mark) = begin(dir.path(), &SystemFiles, &mut none);
    assert_eq!(last, LastRun::Clean);
    assert!(mark.is_some());
    assert_eq!(mark_text(dir.path()), "running\n");
}

#[test]
fn a_clean_quit_leaves_the_next_start_clean_and_lets_go_of_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles, &mut none);
    mark.unwrap().close(&SystemFiles, &mut none);
    assert_eq!(mark_text(dir.path()), "closed\n");
    // No wait: the lock went with the close.
    let waits = Cell::new(0);
    let (last, mark) = begin(dir.path(), &SystemFiles, &mut |_| {
        waits.set(waits.get() + 1)
    });
    assert_eq!(last, LastRun::Clean);
    assert!(mark.is_some());
    assert_eq!(waits.get(), 0);
}

#[test]
fn a_run_that_ends_without_its_quit_makes_the_next_start_unclean() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles, &mut none);
    // A kill: the lock goes with the process, and no quit wrote `closed`.
    drop(mark);
    let (last, mark) = begin(dir.path(), &SystemFiles, &mut none);
    assert_eq!(last, LastRun::Unclean);
    // That start marks itself running; a crash during it counts at the start after.
    drop(mark);
    let (last, _mark) = begin(dir.path(), &SystemFiles, &mut none);
    assert_eq!(last, LastRun::Unclean);
}

#[test]
fn a_second_start_touches_nothing_and_does_not_trip_the_mark() {
    let dir = tempfile::tempdir().unwrap();
    let (_, first) = begin(dir.path(), &SystemFiles, &mut none);
    let first = first.unwrap();
    let waits = Cell::new(0);
    let (second, nothing) = begin(dir.path(), &SystemFiles, &mut |_| {
        waits.set(waits.get() + 1)
    });
    assert_eq!(second, LastRun::OtherInstance);
    assert!(nothing.is_none());
    assert_eq!(waits.get(), LOCK_TRIES - 1, "it waited the whole window");
    assert_eq!(mark_text(dir.path()), "running\n");
    // The first quits cleanly: the start after it is clean, the second start notwithstanding.
    first.close(&SystemFiles, &mut none);
    let (last, _mark) = begin(dir.path(), &SystemFiles, &mut none);
    assert_eq!(last, LastRun::Clean);
}

#[test]
fn a_start_while_the_last_run_is_still_closing_waits_and_starts_normally() {
    let dir = tempfile::tempdir().unwrap();
    let (_, first) = begin(dir.path(), &SystemFiles, &mut none);
    // An update's restart: the new run starts while the old one has yet to finish its quit,
    // which it does (writing `closed`, letting go of the lock) inside the window.
    let mut closing = first;
    let (last, mark) = begin(dir.path(), &SystemFiles, &mut |_| {
        if let Some(old) = closing.take() {
            old.close(&SystemFiles, &mut none);
        }
    });
    assert_eq!(last, LastRun::Clean);
    assert!(mark.is_some());
    assert_eq!(mark_text(dir.path()), "running\n");
}

#[test]
fn a_closed_mark_reads_clean() {
    let dir = tempfile::tempdir().unwrap();
    SystemFiles
        .write_atomic(&dir.path().join(MARK_FILE), b"closed\n")
        .unwrap();
    let (last, _mark) = begin(dir.path(), &SystemFiles, &mut none);
    assert_eq!(last, LastRun::Clean);
}

/// Files whose writes are refused `refusals` times, as a rename over a file an antivirus holds
/// open is on Windows, then go to the disk.
struct Refusing {
    refusals: Cell<u32>,
}

impl Files for Refusing {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        SystemFiles.read(path)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.refusals.get() > 0 {
            self.refusals.set(self.refusals.get() - 1);
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        SystemFiles.write_atomic(path, bytes)
    }
}

#[test]
fn a_refused_quit_write_is_tried_again() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles, &mut none);
    let files = Refusing {
        refusals: Cell::new(2),
    };
    let waits = Cell::new(0);
    mark.unwrap()
        .close(&files, &mut |_| waits.set(waits.get() + 1));
    assert_eq!(mark_text(dir.path()), "closed\n");
    assert_eq!(waits.get(), 2);
}

#[test]
fn a_write_refused_three_times_leaves_the_mark_running() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mark) = begin(dir.path(), &SystemFiles, &mut none);
    let files = Refusing {
        refusals: Cell::new(3),
    };
    mark.unwrap().close(&files, &mut none);
    assert_eq!(mark_text(dir.path()), "running\n");
}
