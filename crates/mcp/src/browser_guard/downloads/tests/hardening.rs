//! The security review's findings on the copy: errors that mean "no such call here", a copy
//! swapped before its rename, leftovers of a killed guard, transient mark errors, notes of a
//! guard's last sweep, and a download with no free name.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;

use super::{Mover, NONE, names, scratch, sweep};
use crate::browser_guard::downloads::deliver::Failed;
use crate::browser_guard::downloads::saved::{Saved, note, queue};
use crate::browser_guard::downloads::{How, Sweeper, deliver};
use crate::write;

/// A move that always crosses devices.
fn crosses(_: &write::Folder, _: &Path, _: &std::ffi::OsStr) -> io::Result<()> {
    Err(io::ErrorKind::CrossesDevices.into())
}

/// A sweeper with no wait, recording what it tells the agent.
fn sweeper(staging: &Path, downloads: &Path, how: How) -> (Sweeper, Arc<Saved>) {
    let saved = Arc::new(Saved::default());
    let mut sweeper = Sweeper::new(staging.to_owned(), downloads.to_owned(), 0);
    sweeper.saved = Some(Arc::clone(&saved));
    sweeper.how = how;
    (sweeper, saved)
}

/// The review's M1: `ENOTSUP` (45 on macOS, apart from `EOPNOTSUPP`, 102) from a hard link means
/// "no hard links here" too, so exFAT, FAT and SMB take the copy.
#[cfg(unix)]
#[test]
fn not_supported_in_either_spelling_takes_the_copy() {
    for code in [nix::libc::ENOTSUP, nix::libc::EOPNOTSUPP, nix::libc::EPERM] {
        assert!(
            write::needs_copy(&io::Error::from_raw_os_error(code)),
            "{code}"
        );
    }
    assert!(!write::needs_copy(&io::Error::from_raw_os_error(
        nix::libc::EACCES
    )));
}

/// The review's M2: the agent swaps its own file in for the copy's temporary file before the
/// rename. The guard renames by name, finds the file at the new name is not the copy it made,
/// and fails the delivery: nothing is told as saved, and the download stays in staging.
#[cfg(unix)]
#[test]
fn a_copy_swapped_before_its_rename_is_not_told_as_saved() {
    let (dir, staging, downloads) = scratch("hard-swap");
    write::write(&staging.join("report.pdf"), "download").unwrap();
    let how = How {
        mover: crosses,
        ..How::default()
    };
    let mut looks = 0;
    let delivered = deliver(
        &staging.join("report.pdf"),
        &downloads,
        "report.pdf",
        how,
        |_: &Path| {
            looks += 1;
            // The first look is the move's; the second the copy's rename.
            if looks != 2 {
                return;
            }
            let temporary = names(&downloads)
                .into_iter()
                .find(|name| name.starts_with(".clauseo-dl-"))
                .unwrap();
            let agents = dir.join("agents");
            write::write(&agents, "the agent's own").unwrap();
            write::replace(&agents, &downloads.join(temporary)).unwrap();
        },
    );
    assert!(matches!(delivered, Err(Failed::Copy(_))));
    assert_eq!(names(&staging), ["report.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}

/// The same swap through the sweep: the agent hears nothing, and after the copy's tries the file
/// is held and told as not saved.
#[cfg(unix)]
#[test]
fn a_swapped_copy_is_never_told_as_saved_by_the_sweep() {
    thread_local! {
        static DOWNLOADS: std::cell::RefCell<std::path::PathBuf> =
            std::cell::RefCell::default();
    }
    /// A mark that swaps the temporary file (the copy, named by its handle's inode) for the
    /// agent's own, by name, as soon as it is marked.
    fn swapping(_: &File, copy: &File) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let folder = DOWNLOADS.with_borrow(Clone::clone);
        let ino = copy.metadata()?.ino();
        for entry in std::fs::read_dir(&folder)?.flatten() {
            if entry.metadata()?.ino() == ino {
                let agents = folder.with_file_name("agents");
                write::write(&agents, "the agent's own")?;
                write::replace(&agents, &entry.path())?;
            }
        }
        Ok(())
    }
    let (dir, staging, downloads) = scratch("hard-swap-sweep");
    DOWNLOADS.with_borrow_mut(|path| path.clone_from(&downloads));
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: crosses,
        mark: swapping,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    for _ in 0..3 {
        sweeper.sweep();
    }
    let told = saved.take();
    assert!(!told.contains(&note("a.pdf")), "{told:?}");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("could not copy it"), "{told:?}");
    assert_eq!(names(&staging), ["a.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}

/// The review's M3: a mover that starts removes the temporary files a killed guard's copy left,
/// in the downloads folder and the fallback folder: plain files with exactly that name. A link
/// of that name, its target, a folder of that name and near names stay.
#[test]
fn a_starting_mover_removes_leftovers_of_a_killed_copy() {
    let (dir, staging, downloads) = scratch("hard-leftovers");
    let fallback = dir.join("fallback");
    write::create_dir_all(&fallback).unwrap();
    let left = ".clauseo-dl-0123456789abcdef";
    write::write(&downloads.join(left), "half a copy").unwrap();
    write::write(&fallback.join(left), "half a copy").unwrap();
    let kept = [
        ".clauseo-dl-0123456789ABCDEF",
        ".clauseo-dl-0123",
        ".clauseo-dl-0123456789abcdef0",
        "report.pdf",
    ];
    for name in kept {
        write::write(&downloads.join(name), "kept").unwrap();
    }
    write::create_dir_all(&downloads.join(".clauseo-dl-fedcba9876543210")).unwrap();
    #[cfg(unix)]
    let outside = dir.join("outside.txt");
    #[cfg(unix)]
    {
        write::write(&outside, "outside").unwrap();
        write::symlink(&outside, &downloads.join(".clauseo-dl-1111111111111111")).unwrap();
    }
    let mover = Mover::start(
        staging,
        downloads.clone(),
        Some(fallback.clone()),
        Arc::default(),
        Arc::default(),
    );
    mover.finish();
    let mut expected: Vec<&str> = kept.to_vec();
    expected.push(".clauseo-dl-fedcba9876543210");
    #[cfg(unix)]
    expected.push(".clauseo-dl-1111111111111111");
    expected.sort_unstable();
    assert_eq!(names(&downloads), expected);
    assert_eq!(names(&fallback), NONE);
    #[cfg(unix)]
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "outside");
    write::remove_dir_all(&dir).unwrap();
}

/// A mark that fails with a busy file, as a scanner holding the new copy would.
fn busy(_: &File, _: &File) -> io::Result<()> {
    #[cfg(unix)]
    let code = nix::libc::EBUSY;
    #[cfg(windows)]
    let code = 32; // ERROR_SHARING_VIOLATION
    Err(io::Error::from_raw_os_error(code))
}

/// A mark the drive does not support.
fn not_supported(_: &File, _: &File) -> io::Result<()> {
    #[cfg(unix)]
    let code = nix::libc::ENOTSUP;
    #[cfg(windows)]
    let code = 50; // ERROR_NOT_SUPPORTED
    Err(io::Error::from_raw_os_error(code))
}

/// The review's low item on deliver.rs: a mark that fails for a passing reason is tried again,
/// then held as a failed copy; it never sends the file to the fallback folder, whose notice
/// would blame the drive.
#[test]
fn a_transient_mark_error_is_retried_not_sent_to_the_fallback() {
    let (dir, staging, downloads) = scratch("hard-busy");
    let fallback = dir.join("fallback");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: crosses,
        mark: busy,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.fallback = Some(fallback.clone());
    sweeper.sweep();
    sweeper.sweep();
    assert_eq!(saved.take(), Vec::<String>::new());
    sweeper.sweep();
    sweeper.sweep();
    let told = saved.take();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("could not copy it"), "{told:?}");
    assert!(!fallback.exists());
    assert_eq!(names(&downloads), NONE);
    assert_eq!(names(&staging), ["a.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}

/// A mark the drive does not support at all sends the file to the fallback folder.
#[test]
fn a_mark_the_drive_does_not_support_goes_to_the_fallback() {
    thread_local! {
        static MOVES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static MARKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    /// Crosses devices to the matter's drive; a real move to the fallback folder.
    fn crosses_once(folder: &write::Folder, from: &Path, name: &std::ffi::OsStr) -> io::Result<()> {
        MOVES.set(MOVES.get() + 1);
        if MOVES.get() == 1 {
            return crosses(folder, from, name);
        }
        folder.move_new(from, name)
    }
    fn first_unsupported(source: &File, copy: &File) -> io::Result<()> {
        MARKS.set(MARKS.get() + 1);
        if MARKS.get() == 1 {
            return not_supported(source, copy);
        }
        Ok(())
    }
    let (dir, staging, downloads) = scratch("hard-unsupported");
    let fallback = dir.join("fallback");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: crosses_once,
        mark: first_unsupported,
    };
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.fallback = Some(fallback.clone());
    sweeper.sweep();
    assert_eq!(names(&fallback), ["a.pdf"]);
    assert!(saved.take()[0].starts_with("Saved a.pdf outside the matter"));
    write::remove_dir_all(&dir).unwrap();
}

/// The review's low item on reaper.rs: what a last sweep (the reaper's, or the guard's at its
/// exit) would have told is queued in the guard's `--cwd`, and the next guard there tells it,
/// once.
#[test]
fn a_last_sweeps_notes_are_queued_for_the_next_guard() {
    let (dir, staging, downloads) = scratch("hard-queue");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let untold = sweep(&staging, &downloads, None);
    assert_eq!(untold, [note("a.pdf")]);
    let cwd = dir.join("cwd");
    write::create_dir_all(&cwd).unwrap();
    queue(&cwd, &untold);
    queue(
        &cwd,
        &["Saved b.pdf outside the matter. Its path: /x/b.pdf".to_owned()],
    );
    queue(&cwd, &[]);
    assert_eq!(names(&cwd).len(), 2, "{:?}", names(&cwd));
    // A file that only looks like one, and a link, are left.
    write::write(&cwd.join(".clauseo-untold-0123"), "[\"no\"]").unwrap();
    let next = Saved::queued_in(cwd.clone());
    let mut told = next.take();
    told.sort();
    assert_eq!(
        told,
        [
            note("a.pdf"),
            "Saved b.pdf outside the matter. Its path: /x/b.pdf".to_owned()
        ]
    );
    assert_eq!(next.take(), Vec::<String>::new());
    assert_eq!(names(&cwd), [".clauseo-untold-0123"]);
    // Through a tool result too.
    queue(&cwd, &untold);
    let line = br#"{"jsonrpc":"2.0","id":1,"result":{"content":[]}}"#;
    let with = next.note_into(line).unwrap();
    assert!(
        String::from_utf8(with)
            .unwrap()
            .contains("Saved a.pdf to downloads/")
    );
    write::remove_dir_all(&dir).unwrap();
}

/// A move that finds every name taken, as a downloads folder filled by the agent would.
fn always_taken(_: &write::Folder, _: &Path, _: &std::ffi::OsStr) -> io::Result<()> {
    Err(io::ErrorKind::AlreadyExists.into())
}

/// The review's low item on the name search: a download that finds no free name is held for
/// good and told once, not looked for again at every sweep.
#[test]
fn a_download_with_no_free_name_is_held_and_told_once() {
    let (dir, staging, downloads) = scratch("hard-no-name");
    write::write(&staging.join("a.pdf"), "download").unwrap();
    let how = How {
        mover: always_taken,
        ..How::default()
    };
    let failed = deliver(&staging.join("a.pdf"), &downloads, "a.pdf", how, |_| {});
    assert!(matches!(failed, Err(Failed::NoFreeName)));
    let (mut sweeper, saved) = sweeper(&staging, &downloads, how);
    sweeper.sweep();
    sweeper.sweep();
    let told = saved.take();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("under every name it could take"),
        "{told:?}"
    );
    assert_eq!(names(&staging), ["a.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}
