//! Whether a sign-in's login run could have written the home's login file, so that a login that
//! did not confirm is removed only when it is that run's ([`LoginRun::may_remove`]): a login
//! process was spawned, and the file changed since just before it (it appeared, or its file id,
//! size or modified time is not the one seen then). A file the run never touched is the login
//! that was there before (a lawyer's own, signed in earlier); a status check that is slow,
//! offline or says signed out must never sign that out.
//!
//! The file is only ever stat'ed, never opened for its contents (`symlink_metadata`, and
//! [`crate::fileid::file_id`]'s access-0 handle on Windows).

use std::path::Path;
use std::time::SystemTime;

/// What a stat of the home's login file said: its id when one could be read, its size and its
/// modified time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    id: Option<(u64, u64)>,
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    /// The home's login file now, `None` when there is none (or it cannot be stat'ed).
    pub(super) fn of(codex_home: &Path) -> Option<Self> {
        let path = crate::signin::reuse::login_file(codex_home);
        let meta = std::fs::symlink_metadata(&path).ok()?;
        Some(Self {
            id: crate::fileid::file_id(&path).ok().flatten(),
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// A sign-in's login run, for the removal of a login that did not confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoginRun {
    /// The login file just before the login was started.
    before: Option<Stamp>,
    /// Whether a login process was spawned (a run stopped before it started spawns none).
    spawned: bool,
}

impl LoginRun {
    /// The run that saw `before` just before its login started, and spawned one or not.
    pub(super) fn new(before: Option<Stamp>, spawned: bool) -> Self {
        Self { before, spawned }
    }

    /// Whether the home's login file may be removed as this run's unconfirmed login: only when
    /// a login process was spawned and the file changed since [`LoginRun::before`].
    pub(super) fn may_remove(&self, codex_home: &Path) -> bool {
        self.spawned && self.changed(Stamp::of(codex_home).as_ref())
    }

    /// Whether `now` is not the file seen before: it appeared, went, or another id, size or
    /// modified time.
    fn changed(&self, now: Option<&Stamp>) -> bool {
        self.before.as_ref() != now
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{LoginRun, Stamp};
    use crate::testing::TempDir;

    fn file(home: &Path) -> PathBuf {
        crate::signin::reuse::login_file(home)
    }

    fn write(home: &Path, contents: &[u8]) {
        crate::signin::reuse::write_for_tests(&file(home), contents);
    }

    /// Test (2): no login process spawned, so the file is kept whatever happened to it: there
    /// before and unchanged, there before and replaced, or appeared since.
    #[test]
    fn no_login_spawned_keeps_the_file_whatever_happened() {
        let home = TempDir::new("written-none").unwrap();
        write(home.path(), b"{}");
        let run = LoginRun {
            before: Stamp::of(home.path()),
            spawned: false,
        };
        assert!(!run.may_remove(home.path()), "an unchanged file went");
        write(home.path(), b"{\"another\": \"dummy, never a credential\"}");
        assert!(!run.may_remove(home.path()), "a replaced file went");

        let empty = TempDir::new("written-none-empty").unwrap();
        let run = LoginRun {
            before: Stamp::of(empty.path()),
            spawned: false,
        };
        write(empty.path(), b"{}");
        assert!(!run.may_remove(empty.path()), "a file that appeared went");
    }

    /// Tests (1) and (3) at the stat: a spawned login removes a file that changed (replaced, or
    /// appeared), never one that is as it was.
    #[test]
    fn a_spawned_login_removes_only_a_file_that_changed() {
        let home = TempDir::new("written-spawned").unwrap();
        write(home.path(), b"{}");
        let run = LoginRun {
            before: Stamp::of(home.path()),
            spawned: true,
        };
        assert!(!run.may_remove(home.path()), "an unchanged file went");
        // `write_for_tests` writes a new file and renames it over: another id and size.
        write(home.path(), b"{\"another\": \"dummy, never a credential\"}");
        assert!(run.may_remove(home.path()));

        let empty = TempDir::new("written-spawned-empty").unwrap();
        let run = LoginRun {
            before: Stamp::of(empty.path()),
            spawned: true,
        };
        assert!(
            !run.may_remove(empty.path()),
            "nothing there, nothing changed"
        );
        write(empty.path(), b"{}");
        assert!(run.may_remove(empty.path()));
    }
}
