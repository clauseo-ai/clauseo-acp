//! Unit tests only: a sync client putting back a file the crate just deleted, as `OneDrive` on
//! macOS can about 3 s after the delete, as a dataless placeholder (cloud-only). The hook sits
//! in [`crate::write::remove_file`]; armed for a path, it brings the file back with the bytes it
//! had (a placeholder keeps its size), [`When`] the test says. Per thread, like [`super::fault`]:
//! a test's deletes and walks run on its own thread.
//!
//! Brought back as a placeholder, the file is marked cloud-only by the test's own probe (the
//! `mark` it passes) and every read of it fails as a read under [`super::NoDownload`] fails on
//! the Mac ([`super::fault::fail`] with `Deadlock`). Brought back downloaded, it is a plain file.

use std::cell::RefCell;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// When an armed file comes back after its delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    /// Right after the delete, before anything else runs: before the walk that follows.
    Immediately,
    /// At the next [`probed`] call, which the test's probe makes for each file a walk
    /// classifies: during the walk, after it started.
    AtNextProbe,
}

/// How an armed file comes back: as a placeholder, which the test's probe is told about by
/// `mark` (given the absolute path), or downloaded (`None`).
pub(crate) type Mark = Option<Box<dyn Fn(&Path)>>;

struct Armed {
    path: PathBuf,
    when: When,
    mark: Mark,
}

/// A file deleted and waiting for its [`When::AtNextProbe`], with its bytes.
struct Due {
    path: PathBuf,
    bytes: Vec<u8>,
    mark: Mark,
}

thread_local! {
    static ARMED: RefCell<Vec<Armed>> = const { RefCell::new(Vec::new()) };
    static DUE: RefCell<Vec<Due>> = const { RefCell::new(Vec::new()) };
}

/// Brings `path` (absolute) back after its next delete through [`crate::write::remove_file`],
/// at `when`, as `mark` says. Once only.
pub(crate) fn on_remove(path: &Path, when: When, mark: Mark) {
    ARMED.with_borrow_mut(|armed| {
        armed.push(Armed {
            path: path.to_owned(),
            when,
            mark,
        });
    });
}

/// Before the delete of `path`: its bytes when a return is armed for it (and disarms it).
pub(crate) fn removing(path: &Path) -> Option<(Vec<u8>, When, Mark)> {
    let armed = ARMED.with_borrow_mut(|armed| {
        let at = armed.iter().position(|one| one.path == path)?;
        Some(armed.remove(at))
    })?;
    let bytes = std::fs::read(path).expect("the armed file's bytes before its delete");
    Some((bytes, armed.when, armed.mark))
}

/// After `path` was deleted: brings it back now, or at the next [`probed`], by `when`.
pub(crate) fn removed(path: &Path, (bytes, when, mark): (Vec<u8>, When, Mark)) {
    match when {
        When::Immediately => bring_back(path, &bytes, mark.as_deref()),
        When::AtNextProbe => DUE.with_borrow_mut(|due| {
            due.push(Due {
                path: path.to_owned(),
                bytes,
                mark,
            });
        }),
    }
}

/// One probe call by a walk: brings back every file due at it.
pub(crate) fn probed() {
    for due in DUE.take() {
        bring_back(&due.path, &due.bytes, due.mark.as_deref());
    }
}

/// Whether a file is still waiting for its [`When::AtNextProbe`].
pub(crate) fn pending() -> bool {
    DUE.with_borrow(|due| !due.is_empty())
}

fn bring_back(path: &Path, bytes: &[u8], mark: Option<&dyn Fn(&Path)>) {
    let back = crate::write::create_new(path).and_then(|mut file| file.write_all(bytes));
    back.expect("the test's file brought back");
    if let Some(mark) = mark {
        mark(path);
        super::fault::fail(path, 0, io::ErrorKind::Deadlock);
    }
}
