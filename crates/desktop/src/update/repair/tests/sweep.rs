//! The start's sweep of `updates/<version>/repair-*`: plain folders of plain files go; links,
//! anything unexpected and other names stay.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up writes and removes files in its scratch folder; no write module here"
)]

use std::path::Path;

use super::super::sweep::{Swept, sweep};
use crate::test_support::Scratch;

/// `updates/<version>/<name>/` holding one installer.
fn attempt(updates: &Path, version: &str, name: &str) -> std::path::PathBuf {
    let folder = updates.join(version).join(name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Clauseo_0.4.1_x64-setup.exe"), b"installer").unwrap();
    folder
}

/// Every `repair-*` folder of plain files goes, and its version's folder once empty; a folder
/// of another name, a version folder's other files and a non-version folder stay.
#[test]
fn last_runs_attempts_are_swept() {
    let scratch = Scratch::new("repair-sweep", &[]);
    let updates = scratch.path().join("updates");
    let one = attempt(&updates, "0.4.1", "repair-0001");
    let two = attempt(&updates, "0.4.1", "repair-0002");
    let other = attempt(&updates, "0.4.0", "repair-0003");
    let kept = attempt(&updates, "0.4.2", "not-a-repair");
    let elsewhere = attempt(&updates, "notes", "repair-0004");
    let mut swept = sweep(&updates);
    swept.sort_by_key(|outcome| format!("{outcome:?}"));
    assert_eq!(swept, [Swept::Removed, Swept::Removed, Swept::Removed]);
    for gone in [&one, &two, &other] {
        assert!(!gone.exists(), "{}", gone.display());
    }
    assert!(!updates.join("0.4.1").exists());
    assert!(kept.exists());
    assert!(elsewhere.exists());
    assert_eq!(sweep(&scratch.path().join("nowhere")), []);
}

/// A folder holding a folder is left as it is.
#[test]
fn a_folder_holding_more_than_files_is_left() {
    let scratch = Scratch::new("repair-sweep-nested", &[]);
    let updates = scratch.path().join("updates");
    let folder = attempt(&updates, "0.4.1", "repair-0001");
    std::fs::create_dir(folder.join("inside")).unwrap();
    assert_eq!(sweep(&updates), [Swept::Unexpected]);
    assert!(folder.join("Clauseo_0.4.1_x64-setup.exe").exists());
}

/// A `repair-*` link to a folder elsewhere, and a link to a file inside one, are never
/// followed: what they point at stays.
#[cfg(unix)]
#[test]
fn a_link_is_never_followed() {
    let scratch = Scratch::new("repair-sweep-link", &[]);
    let updates = scratch.path().join("updates");
    let target = scratch.path().join("lawyers-files");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("brief.docx"), b"keep me").unwrap();
    std::fs::create_dir_all(updates.join("0.4.1")).unwrap();
    std::os::unix::fs::symlink(&target, updates.join("0.4.1").join("repair-link")).unwrap();
    assert_eq!(sweep(&updates), [Swept::Link]);
    assert!(target.join("brief.docx").exists());

    let folder = attempt(&updates, "0.4.2", "repair-0001");
    std::os::unix::fs::symlink(target.join("brief.docx"), folder.join("linked")).unwrap();
    let mut swept = sweep(&updates);
    swept.sort_by_key(|outcome| format!("{outcome:?}"));
    assert_eq!(swept, [Swept::Link, Swept::Unexpected]);
    assert!(target.join("brief.docx").exists());
}

/// `updates/` itself as a link to a folder elsewhere is never walked: what it points at stays.
#[cfg(unix)]
#[test]
fn a_linked_updates_folder_is_never_followed() {
    let scratch = Scratch::new("repair-sweep-linked-updates", &[]);
    let elsewhere = scratch.path().join("elsewhere");
    let kept = attempt(&elsewhere, "0.4.1", "repair-0001");
    let updates = scratch.path().join("updates");
    std::os::unix::fs::symlink(&elsewhere, &updates).unwrap();
    assert_eq!(sweep(&updates), []);
    assert!(kept.join("Clauseo_0.4.1_x64-setup.exe").exists());
}
