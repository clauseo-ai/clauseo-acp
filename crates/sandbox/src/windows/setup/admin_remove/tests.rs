//! `--remove`'s deletion of the admin folder on NTFS (Windows only), in a folder of the test's
//! own: everything inside goes, a link without what it points at, and the running copy is
//! parked and handed to the restart-time delete with the folders, in order.

use std::path::{Path, PathBuf};

use super::{Restart, clear, parked_name};
use crate::test_support::TempDir;
use crate::windows::admin_copy::{Found, os};

/// A Sandbox stand-in: the copy, its signature, a staging folder with a file, a stray DLL, and
/// a junction to a folder outside that must survive.
fn sandbox(dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let outside = dir.file(r"outside\keep.txt", "the lawyer's");
    let exe = dir.file(r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe", "MZ");
    dir.file(r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe.sig", "sig");
    dir.file(r"pf\Clauseo\Sandbox\staging\new.exe", "MZ new");
    dir.file(r"pf\Clauseo\Sandbox\version.dll", "planted");
    let sandbox = exe.parent().unwrap().to_path_buf();
    crate::windows::test_junction::junction(&sandbox.join("link"), outside.parent().unwrap())
        .unwrap();
    (sandbox, exe, outside)
}

#[test]
fn a_copy_not_running_goes_with_its_folder_and_a_link_is_not_followed() {
    let dir = TempDir::new("admin-remove-idle");
    let (sandbox, exe, outside) = sandbox(&dir);
    let mut scheduled = Vec::new();
    let mut schedule = |path: &Path| {
        scheduled.push(path.to_path_buf());
        Ok(())
    };
    let app = dir.path().join(r"pf\Clauseo");
    let mut rename = |from: &Path, to: &Path| std::fs::rename(from, to);
    let mut fs = Restart {
        rename: &mut rename,
        schedule: &mut schedule,
    };
    clear(&sandbox, &exe, false, &app, &mut fs).unwrap();
    assert_eq!(os::found(&sandbox).unwrap(), Found::Missing);
    assert!(scheduled.is_empty(), "{scheduled:?}");
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "the lawyer's");
    assert_eq!(os::found(&app).unwrap(), Found::Folder);
}

#[test]
fn the_running_copy_is_parked_and_scheduled_with_the_folders_in_order() {
    let dir = TempDir::new("admin-remove-running");
    let (sandbox, exe, outside) = sandbox(&dir);
    let mut scheduled = Vec::new();
    let mut schedule = |path: &Path| {
        scheduled.push(path.to_path_buf());
        Ok(())
    };
    let app = dir.path().join(r"pf\Clauseo");
    let mut rename = |from: &Path, to: &Path| std::fs::rename(from, to);
    let mut fs = Restart {
        rename: &mut rename,
        schedule: &mut schedule,
    };
    clear(&sandbox, &exe, true, &app, &mut fs).unwrap();
    let parked = parked_name(&exe, std::process::id());
    assert_eq!(scheduled, [parked.clone(), sandbox.clone(), app]);
    assert_eq!(os::found(&exe).unwrap(), Found::Missing);
    assert_eq!(os::found(&parked).unwrap(), Found::File);
    let left: Vec<_> = std::fs::read_dir(&sandbox)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(left, [parked], "only the parked copy stays");
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "the lawyer's");
}

#[test]
fn a_running_copy_that_cant_be_renamed_is_left_and_nothing_is_scheduled() {
    let dir = TempDir::new("admin-remove-unrenamed");
    let (sandbox, exe, _outside) = sandbox(&dir);
    let mut scheduled = Vec::new();
    let mut schedule = |path: &Path| {
        scheduled.push(path.to_path_buf());
        Ok(())
    };
    let mut rename =
        |_: &Path, _: &Path| -> std::io::Result<()> { Err(std::io::Error::from_raw_os_error(5)) };
    let mut fs = Restart {
        rename: &mut rename,
        schedule: &mut schedule,
    };
    let app = dir.path().join(r"pf\Clauseo");
    let error = clear(&sandbox, &exe, true, &app, &mut fs).unwrap_err();
    assert_eq!(
        error.raw_os_error(),
        Some(5),
        "the rename's error, for the step's line"
    );
    assert!(
        scheduled.is_empty(),
        "the canonical name is never scheduled: {scheduled:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&exe).unwrap(),
        "MZ",
        "left in place"
    );
}
