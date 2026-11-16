//! The secret file's retry while another program has it open, and, on Windows, what the one
//! handle asks for: `DELETE` access, and no sharing but reading while the guard holds it.

use std::cell::Cell;
use std::io;

use super::{TRIES, first_free};

/// An error the tests' `busy` accepts: a stand-in for a sharing violation on every OS.
fn busy() -> io::Error {
    io::Error::from(io::ErrorKind::ResourceBusy)
}

fn is_busy(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::ResourceBusy
}

/// A busy open is tried again after a pause until it succeeds; one that stays busy is given up
/// after the last try, with the busy error; any other error is returned at once.
#[test]
fn a_busy_open_is_tried_again_until_the_last_try() {
    let pauses = Cell::new(0);
    let opens = Cell::new(0);
    let freed_on_third = first_free(
        || {
            opens.set(opens.get() + 1);
            if opens.get() < 3 {
                Err(busy())
            } else {
                Ok("file")
            }
        },
        is_busy,
        TRIES,
        &mut || pauses.set(pauses.get() + 1),
    );
    assert_eq!(freed_on_third.unwrap(), "file");
    assert_eq!((opens.get(), pauses.get()), (3, 2));

    let (pauses, opens) = (Cell::new(0), Cell::new(0_u32));
    let kept = first_free(
        || -> io::Result<()> {
            opens.set(opens.get() + 1);
            Err(busy())
        },
        is_busy,
        TRIES,
        &mut || pauses.set(pauses.get() + 1),
    );
    assert!(is_busy(&kept.unwrap_err()));
    assert_eq!((opens.get(), pauses.get()), (TRIES, TRIES - 1));

    let (pauses, opens) = (Cell::new(0), Cell::new(0));
    let other = first_free(
        || -> io::Result<()> {
            opens.set(opens.get() + 1);
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        },
        is_busy,
        TRIES,
        &mut || pauses.set(pauses.get() + 1),
    );
    assert_eq!(other.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!((opens.get(), pauses.get()), (1, 0));
}

#[cfg(windows)]
mod on_windows {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use windows::Win32::Foundation::ERROR_SHARING_VIOLATION;

    use super::super::{Refused, TRIES, take_with};

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";

    /// A folder of the test's own under the temp folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "clauseo-mcp-secret-share-{test}-{}",
                std::process::id()
            ));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn secret(&self) -> PathBuf {
            let file = self.0.join("proxy-secret");
            std::fs::write(&file, SECRET).unwrap();
            file
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    fn is_sharing_violation(error: &std::io::Error) -> bool {
        error.raw_os_error() == i32::try_from(ERROR_SHARING_VIOLATION.0).ok()
    }

    /// Another program's handle that reads, sharing reading and writing but not deleting.
    fn held_without_delete(file: &Path) -> std::fs::File {
        crate::write::open_shared_without_delete(file).unwrap()
    }

    /// A holder that lets go inside the waiting window: the guard's next try opens the file,
    /// reads it, empties it and deletes it.
    #[test]
    fn a_holder_that_lets_go_inside_the_window_is_waited_for() {
        let scratch = Scratch::new("lets-go");
        let file = scratch.secret();
        let holder = RefCell::new(Some(held_without_delete(&file)));
        let pauses = Cell::new(0);
        let taken = take_with(
            &file,
            128,
            &mut || {
                pauses.set(pauses.get() + 1);
                if pauses.get() == 2 {
                    drop(holder.borrow_mut().take());
                }
            },
            &mut || {},
        )
        .unwrap();
        assert_eq!(taken.bytes, SECRET);
        assert!(taken.undeleted.is_none(), "{:?}", taken.undeleted);
        assert_eq!(pauses.get(), 2);
        assert!(file.symlink_metadata().is_err(), "not deleted");
    }

    /// A holder that keeps the file through every try (one sharing reading and writing but not
    /// deleting, so the guard's `DELETE` access is what fails): refused as busy, bytes kept.
    #[test]
    fn a_holder_that_keeps_the_file_is_refused_as_busy() {
        let scratch = Scratch::new("keeps");
        let file = scratch.secret();
        let holder = held_without_delete(&file);
        let pauses = Cell::new(0);
        let refused = take_with(&file, 128, &mut || pauses.set(pauses.get() + 1), &mut || {});
        assert_eq!(refused.unwrap_err(), Refused::Busy);
        assert_eq!(pauses.get(), TRIES - 1);
        drop(holder);
        assert_eq!(std::fs::read(&file).unwrap(), SECRET);
    }

    /// While the guard holds the file, between its read and the emptying, another program can
    /// neither open it for writing nor delete it: both fail with a sharing violation.
    #[test]
    fn while_the_guard_holds_the_file_nobody_else_writes_or_deletes_it() {
        let scratch = Scratch::new("held");
        let file = scratch.secret();
        let seen = RefCell::new(Vec::new());
        let taken = take_with(&file, 128, &mut || {}, &mut || {
            let write = std::fs::OpenOptions::new().write(true).open(&file);
            let delete = std::fs::remove_file(&file);
            seen.borrow_mut().push((write.err(), delete.err()));
        })
        .unwrap();
        assert_eq!(taken.bytes, SECRET);
        let seen = seen.into_inner();
        let [(Some(write), Some(delete))] = seen.as_slice() else {
            panic!("another program wrote or deleted the held file: {seen:?}");
        };
        assert!(is_sharing_violation(write), "{write}");
        assert!(is_sharing_violation(delete), "{delete}");
    }
}
