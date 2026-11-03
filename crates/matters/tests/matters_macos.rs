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
}
