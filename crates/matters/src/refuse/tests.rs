use std::path::{Path, PathBuf};

use super::{check_folder, is_unc, is_unc_text, is_windows_drive_root_text, same_folder};
use crate::test_dir::TestDir;
use crate::{Error, MissingReason, Refusal, write};

#[test]
fn windows_drive_roots_by_text() {
    for root in [r"C:\", r"c:\", r"\\?\C:\", "D:", "z:/"] {
        assert!(is_windows_drive_root_text(root), "{root} is a drive root");
    }
    for other in [
        r"C:\Users",
        r"\\?\C:\x",
        r"\\server\share",
        r"\\?\UNC\server\share\x",
        "/",
        "/Volumes/x",
        r"1:\",
        r"CC:\",
        "",
    ] {
        assert!(
            !is_windows_drive_root_text(other),
            "{other} is not a drive root"
        );
    }
}

#[test]
fn unc_paths_by_text() {
    for unc in [
        r"\\server\share",
        r"\\server\share\matter",
        r"\\?\UNC\server\share\x",
        r"\\?\unc\server\share",
    ] {
        assert!(is_unc_text(unc), "{unc} is UNC");
    }
    for other in [
        r"C:\",
        r"\\?\C:\x",
        r"\\?\C:\",
        r"\\.\pipe\x",
        r"\\\x",
        r"C:\Users",
        "/Users/x",
        "//",
        "",
    ] {
        assert!(!is_unc_text(other), "{other} is not UNC");
    }
    assert!(!is_unc(Path::new("/tmp/matter")));
}

#[test]
fn a_missing_folder_is_folder_missing() {
    let dir = TestDir::new("refuse-missing");
    let err = check_folder(&dir.path().join("gone"), &[]).unwrap_err();
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

#[cfg(unix)]
#[test]
fn the_file_system_root_is_a_drive_root() {
    let err = check_folder(Path::new("/"), &[]).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderRefused {
                reason: Refusal::DriveRoot,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_listed_entry_that_does_not_exist_is_compared_as_written() {
    let dir = TestDir::new("refuse-missing-entry");
    let listed: PathBuf = dir.path().join("not-yet").join("deeper");
    let err = check_folder(dir.path(), std::slice::from_ref(&listed)).unwrap_err();
    assert!(
        matches!(&err, Error::FolderRefused { reason: Refusal::Listed(entry), .. }
            if *entry == listed),
        "{err:?}"
    );
    let sibling = dir.path().join("sibling");
    write::create_dir_all(&sibling).unwrap();
    check_folder(&sibling, &[listed]).unwrap();
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn listed_entries_match_in_any_case() {
    let dir = TestDir::new("refuse-case");
    let home = dir.path().join("Home");
    write::create_dir_all(&home).unwrap();
    // The same folder spelled `HOME`, whether or not the volume ignores case.
    let listed = dir.path().join("HOME");
    let err = check_folder(&home, &[listed]).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderRefused {
                reason: Refusal::Listed(_),
                ..
            }
        ),
        "{err:?}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn volumes_are_drive_roots_on_macos() {
    assert!(super::is_macos_volume_key(
        &super::spellings(Path::new("/Volumes/USB"))[0]
    ));
    assert!(!super::is_macos_volume_key(
        &super::spellings(Path::new("/Volumes/USB/x"))[0]
    ));
    let keys = super::spellings(Path::new("/System/Volumes/Data"));
    assert!(keys.iter().any(|key| super::is_macos_volume_key(key)));
}

#[test]
fn same_folder_compares_canonical_spellings() {
    let dir = TestDir::new("refuse-same");
    let matter = dir.path().join("Matter");
    let sibling = dir.path().join("Sibling");
    write::create_dir_all(&matter).unwrap();
    write::create_dir_all(&sibling).unwrap();
    assert!(same_folder(&matter, &matter));
    assert!(same_folder(&matter, &sibling.join("..").join("Matter")));
    assert!(!same_folder(&matter, &sibling));
    assert!(!same_folder(&matter, &dir.path().join("gone")));
    assert!(!same_folder(
        &dir.path().join("gone"),
        &dir.path().join("gone")
    ));
    #[cfg(any(target_os = "macos", windows))]
    assert!(same_folder(&matter, &dir.path().join("MATTER")));
}
