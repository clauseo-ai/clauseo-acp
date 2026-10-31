//! Unit tests only: make chosen reads in the folder fail, as a read of a file evicted after
//! its stat fails under [`super::NoDownload`] on macOS, on every OS. The faults are per thread,
//! and a test's reads run on its own thread.
//!
//! The hooks sit where the crate reads a file (each read call of `history/blob.rs`, after the
//! open, as on the Mac: the open succeeds and the read fails) and lists a folder (`walk.rs`).
//! On macOS they also record the thread's download policy at each call, so a test can check
//! that every read ran under the guard.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

/// One armed fault.
struct Fault {
    path: PathBuf,
    /// Read calls on `path` that succeed before every later one fails.
    allowed: usize,
    kind: io::ErrorKind,
    /// How many calls failed.
    failed: usize,
}

thread_local! {
    static FAULTS: RefCell<Vec<Fault>> = const { RefCell::new(Vec::new()) };
    /// The file `history/blob.rs` is reading now.
    static READING: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// macOS: the thread's download policy at each read call and listing, in order.
    #[cfg(target_os = "macos")]
    static POLICIES: RefCell<Vec<std::ffi::c_int>> = const { RefCell::new(Vec::new()) };
}

/// macOS: the thread's download policy at each read call and listing so far, and forgets them.
#[cfg(target_os = "macos")]
pub(crate) fn take_policies() -> Vec<std::ffi::c_int> {
    POLICIES.take()
}

/// Makes reads of `path` (absolute: a file, or a folder to list) fail with `kind` after
/// `allowed` read calls succeed.
pub(crate) fn fail(path: &Path, allowed: usize, kind: io::ErrorKind) {
    FAULTS.with_borrow_mut(|faults| {
        faults.push(Fault {
            path: path.to_owned(),
            allowed,
            kind,
            failed: 0,
        });
    });
}

/// How many read calls of `path` failed so far.
pub(crate) fn failed(path: &Path) -> usize {
    FAULTS.with_borrow(|faults| {
        faults
            .iter()
            .filter(|fault| fault.path == path)
            .map(|fault| fault.failed)
            .sum()
    })
}

/// Disarms every fault of this thread.
pub(crate) fn clear() {
    FAULTS.with_borrow_mut(Vec::clear);
}

/// Says which file the next [`read`] calls are for.
pub(crate) fn reading(path: &Path) {
    READING.set(Some(path.to_owned()));
}

/// One read call of the file [`reading`] named: the armed fault's error once its allowance is
/// spent.
pub(crate) fn read() -> io::Result<()> {
    let path = READING.with_borrow(Clone::clone).unwrap_or_default();
    hit(&path)
}

/// One listing of the folder `dir`.
pub(crate) fn read_dir(dir: &Path) -> io::Result<()> {
    hit(dir)
}

fn hit(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    POLICIES.with_borrow_mut(|seen| seen.push(super::macos::thread_policy().unwrap_or(-1)));
    FAULTS.with_borrow_mut(|faults| {
        for fault in faults.iter_mut().filter(|fault| fault.path == path) {
            if fault.allowed == 0 {
                fault.failed += 1;
                return Err(io::Error::from(fault.kind));
            }
            fault.allowed -= 1;
        }
        Ok(())
    })
}
