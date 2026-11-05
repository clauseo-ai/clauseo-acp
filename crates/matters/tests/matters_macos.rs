//! macOS: the place check the host runs before an import, through the public interface, in
//! real folders. `/` and its Data-volume spelling are drive roots; a listed folder and its
//! parents are refused; a folder beside it is not.

#![cfg(target_os = "macos")]
#![expect(
    clippy::disallowed_methods,
    reason = "the macOS tests make their folders with std's writes; clippy.toml bans those calls \
              elsewhere"
)]

/// The tests and their helpers, under `cfg(test)` so clippy treats the helpers as test code.
#[cfg(test)]
mod macos {
    use std::io;
    use std::path::{Path, PathBuf};

    use clauseo_matters::{Error, MissingReason, Refusal, check_place};

    /// A test's own folder, empty when made, canonical (`/private/var/...`), removed on drop.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("clauseo-matters-mac-{name}-{}", std::process::id()));
            if let Err(err) = std::fs::remove_dir_all(&path) {
                assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
            }
            std::fs::create_dir_all(&path).unwrap();
            Self(dunce::canonicalize(&path).unwrap())
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_dir_all(&self.0) {
                tracing::warn!(path = %self.0.display(), error = %err, "test folder left behind");
            }
        }
    }

    /// The refusal in `err`, panicking on any other error.
    fn refusal(err: Error) -> Refusal {
        match err {
            Error::FolderRefused { reason, .. } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// `/` and `/System/Volumes/Data` (the same volume through its firmlink) are refused as drive
    /// roots; a folder under the temp folder is not.
    #[test]
    fn the_root_volume_is_refused_as_a_drive_root() {
        for root in ["/", "/System/Volumes/Data"] {
            let err = check_place(Path::new(root), &[]).unwrap_err();
            assert_eq!(refusal(err), Refusal::DriveRoot, "{root}");
        }
        let dir = TestDir::new("drive-root");
        check_place(dir.path(), &[]).unwrap();
    }

    /// A listed folder and its parents are refused, named by the listed entry, in any case; a
    /// folder beside it is not, and a folder that does not exist is missing.
    #[test]
    fn a_listed_folder_and_its_parents_are_refused() {
        let dir = TestDir::new("listed");
        let listed = dir.path().join("Home");
        let sibling = dir.path().join("Matter");
        std::fs::create_dir_all(&listed).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        for folder in [&listed, &dir.path().join("HOME"), &dir.path().to_owned()] {
            let err = check_place(folder, std::slice::from_ref(&listed)).unwrap_err();
            assert_eq!(
                refusal(err),
                Refusal::Listed(listed.clone()),
                "{}",
                folder.display()
            );
        }
        check_place(&sibling, std::slice::from_ref(&listed)).unwrap();
        let err = check_place(&dir.path().join("gone"), &[]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::FolderMissing {
                    reason: MissingReason::NoFolder,
                    ..
                }
            ),
            "{err:?}"
        );
    }
    /// What [`hold_lock_for_parent`] writes to its stderr once it holds the lock. Matched within
    /// a line and on stderr, so libtest's own output (on stdout; with one test thread its
    /// `test .. ... ` prefix shares the line) never hides it.
    const LOCKED: &str = "clauseo-hold-lock: locked";

    /// Run in a child of the test below: takes a shared `F_SETLK` lock on the whole file
    /// `CLAUSEO_HOLD_LOCK` names, as Word does on a document it has open, writes [`LOCKED`] to
    /// stderr, and keeps it until its stdin closes. Ignored, so it runs only when the test starts
    /// it; without the variable it does nothing.
    #[test]
    #[ignore = "a helper the stale-owner test runs in a child process"]
    fn hold_lock_for_parent() {
        use std::io::Read;

        use nix::fcntl::{FcntlArg, fcntl};
        use nix::libc;

        let Some(target) = std::env::var_os("CLAUSEO_HOLD_LOCK") else {
            return;
        };
        let file = std::fs::File::open(target).unwrap();
        let lock = libc::flock {
            l_start: 0,
            l_len: 0,
            l_pid: 0,
            l_type: libc::F_RDLCK,
            l_whence: libc::c_short::try_from(libc::SEEK_SET).unwrap(),
        };
        fcntl(&file, FcntlArg::F_SETLK(&lock)).unwrap();
        eprintln!("{LOCKED}");
        let mut rest = Vec::new();
        drop(io::stdin().lock().read_to_end(&mut rest));
        drop(file);
    }

    /// The stale owner-file rule on macOS (this binary never calls `test_support`'s pretend
    /// Word, so it reads the real process list; no Word runs on CI): with no Word running, an
    /// owner file whose document no process locks, in a local APFS temp folder, is stale and
    /// shows nothing held. While another process holds a shared `F_SETLK` lock on the document,
    /// as Word does, the same owner file is held. (`F_GETLK` does not report this process's own
    /// locks, so the lock is taken by a child: this test binary again, running
    /// [`hold_lock_for_parent`].)
    #[test]
    fn an_owner_file_is_stale_with_no_word_and_no_lock_and_held_while_another_process_locks_it() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let dir = TestDir::new("stale-owner");
        let root = dir.path();
        let document = root.join("Invoice.docx");
        std::fs::write(&document, b"one").unwrap();
        std::fs::write(
            root.join("~$nvoice.docx"),
            b"owner file left by a crashed Word",
        )
        .unwrap();
        let invoice = clauseo_protocol::MatterPath::new("Invoice.docx").unwrap();
        assert_eq!(
            clauseo_matters::held_open(root, &invoice),
            None,
            "stale: no lock, no Word, a local volume"
        );

        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "macos::hold_lock_for_parent",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env("CLAUSEO_HOLD_LOCK", &document)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let locked = lines
            .by_ref()
            .map_while(Result::ok)
            .any(|line| line.contains(LOCKED));
        assert!(locked, "the child never took the lock");
        assert_eq!(
            clauseo_matters::held_open(root, &invoice),
            Some(clauseo_matters::HeldOpen::OwnerFile(
                "~$nvoice.docx".to_owned()
            )),
            "held: another process locks the document"
        );
        drop(child.stdin.take());
        // Read the rest, so the child's last lines never meet a closed pipe.
        lines.for_each(drop);
        assert!(child.wait().unwrap().success());
    }
}
