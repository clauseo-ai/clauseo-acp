//! The admin copy's checks on NTFS (Windows only): descriptors as Windows writes and reads
//! them. The lock-down and the full chain need an elevated token (to make Administrators the
//! owner), so that test is an `elevated_` one, run by CI's elevated step.

use std::io;
use std::path::{Path, PathBuf};

use super::os::{self, WindowsAdminCopy, lock_down, lock_down_tree};
use super::*;
use crate::test_support::TempDir;
use crate::windows::test_acl::elevated_tests_enabled;
use crate::windows::test_dacl::protect_with;

/// The locked folder's DACL with a Users entry for write and append planted in it.
const PLANTED: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200af;;;BU)";

/// A Program Files stand-in at `root`, read from NTFS, with `recorded` as the record's hash.
struct Rooted {
    root: PathBuf,
    recorded: Option<String>,
}

impl AdminCopyFs for Rooted {
    fn program_files(&self) -> io::Result<PathBuf> {
        Ok(self.root.clone())
    }

    fn found(&self, path: &Path) -> io::Result<Found> {
        os::found(path)
    }

    fn security(&self, path: &Path) -> io::Result<Vec<u8>> {
        os::security(path)
    }

    fn sha256(&self, path: &Path) -> io::Result<String> {
        os::sha256(path)
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        Ok(self.recorded.clone())
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<String>> {
        WindowsAdminCopy.entries(dir)
    }

    fn another_owner(&self) -> io::Result<bool> {
        Ok(false)
    }
}

/// Whether the DACL in `bytes` has an allow entry for Users with a change right.
fn users_can_change(bytes: &[u8]) -> bool {
    let dacl = Descriptor::parse(bytes).unwrap().dacl.unwrap();
    Acl::parse(dacl).unwrap().aces().iter().any(|ace| {
        acl_bytes::kind(ace) == acl_bytes::ALLOWED
            && acl_bytes::sid(ace)
                .and_then(acl_bytes::sid_text)
                .is_some_and(|sid| sid.as_str() == USERS)
            && acl_bytes::mask(ace).is_some_and(|mask| mask & CHANGE_RIGHTS != 0)
    })
}

#[test]
fn a_planted_users_write_reads_back_from_ntfs_and_the_level_is_unsafe() {
    let dir = TempDir::new("admin-copy-planted");
    let folder = dir.dir("Sandbox");
    protect_with(&folder, PLANTED).unwrap();
    let bytes = os::security(&folder).unwrap();
    assert!(
        users_can_change(&bytes),
        "the planted entry reads back with its write bits"
    );
    assert!(!level_safe(&bytes));
    assert_eq!(os::found(&folder).unwrap(), Found::Folder);
}

#[test]
fn the_hash_is_the_files_sha256_and_a_missing_file_is_missing() {
    let dir = TempDir::new("admin-copy-hash");
    let file = dir.file("x.exe", "abc");
    // SHA-256("abc").
    assert_eq!(
        os::sha256(&file).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(os::found(&dir.path().join("none")).unwrap(), Found::Missing);
}

#[test]
#[ignore = "elevated: makes Administrators the owner; CI's elevated step runs it"]
fn elevated_admin_copy_lock_down_reads_back_and_a_planted_users_write_refuses() {
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("admin-copy-elevated");
    let root = dir.path().join("pf");
    let exe = dir.file(
        r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "MZ stand-in",
    );
    assert!(same_path(&exe, &admin_exe(&root)));
    let mut levels = vec![root.clone()];
    for part in ADMIN_DIR {
        let next = levels.last().unwrap().join(part);
        levels.push(next);
    }
    for level in &levels {
        lock_down(level, true).unwrap();
        let bytes = os::security(level).unwrap();
        assert!(
            locked(&bytes, true) && level_safe(&bytes),
            "{}",
            level.display()
        );
    }
    lock_down(&exe, false).unwrap();
    let back = os::security(&exe).unwrap();
    assert!(
        locked(&back, false) && level_safe(&back),
        "the copy reads back locked"
    );
    let recorded = Some(os::sha256(&exe).unwrap());
    let fs = Rooted { root, recorded };
    assert!(check(&fs).is_ok_and(|path| same_path(&path, &exe)));
    // Users given write on the Sandbox folder: unsafe, and the read-back no longer `locked`.
    let sandbox = levels.last().unwrap();
    protect_with(sandbox, PLANTED).unwrap();
    let planted = os::security(sandbox).unwrap();
    assert!(!locked(&planted, true) && !level_safe(&planted));
    assert_eq!(check(&fs), Err(Blocked::AdminCopyUnsafe));
    // A stray beside the copy refuses; the lock-down deletes it and locks what stays.
    let stray = dir.file(r"pf\Clauseo\Sandbox\version.dll", "planted");
    assert_eq!(check(&fs), Err(Blocked::AdminCopyUnsafe));
    lock_down_tree(sandbox, &exe).unwrap();
    assert_eq!(os::found(&stray).unwrap(), Found::Missing);
    assert!(locked(&os::security(sandbox).unwrap(), true));
    assert!(check(&fs).is_ok());
    // A changed copy is changed.
    dir.file(
        r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "MZ another",
    );
    lock_down(&exe, false).unwrap();
    assert_eq!(check(&fs), Err(Blocked::AdminCopyChanged));
}
