use std::ffi::OsString;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use super::{
    Fold, check_folder, fold, is_unc, is_unc_text, is_windows_drive_root_text, same_folder,
    same_or_nested,
};
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

#[test]
fn same_or_nested_holds_for_the_folder_its_parents_and_its_children() {
    let dir = TestDir::new("refuse-nested");
    let matter = dir.path().join("Matter");
    let inside = matter.join("Sub");
    let sibling = dir.path().join("Matter 2");
    write::create_dir_all(&inside).unwrap();
    write::create_dir_all(&sibling).unwrap();
    assert!(same_or_nested(&matter, &matter));
    assert!(same_or_nested(&matter, &inside));
    assert!(same_or_nested(&inside, &matter));
    assert!(same_or_nested(dir.path(), &matter));
    assert!(!same_or_nested(&matter, &sibling));
    // A registered folder that has moved away compares as written.
    assert!(same_or_nested(&matter.join("gone"), &matter));
    assert!(!same_or_nested(&dir.path().join("gone"), &matter));
    // A folder whose name only starts with the other's is beside it, not inside.
    assert!(!same_or_nested(&dir.path().join("Matter-notes"), &matter));
    // A tree not made yet compares as written, and so does a folder inside it.
    let not_yet = matter.join("not-yet");
    assert!(same_or_nested(&not_yet.join("x"), &not_yet));
    #[cfg(any(target_os = "macos", windows))]
    {
        assert!(same_or_nested(&dir.path().join("MATTER"), &inside));
        assert!(same_or_nested(
            &dir.path().join("MATTER").join("sub"),
            &matter
        ));
    }
}

/// Two sibling folders whose names differ only by Unicode form (`ऱ` composed, U+0931, and `र` +
/// nukta U+093C): the refusal matches fold to NFC, so each is "the same or nested" for the
/// other on every OS; the folder relink writes into is matched as the file system finds it, so
/// on ext4 and NTFS, which keep the two apart, the sibling is not the matter's folder.
#[test]
fn a_sibling_in_another_unicode_form_is_refused_but_never_relinked() {
    let dir = TestDir::new("refuse-forms");
    let composed = dir
        .path()
        .join("\u{931}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}");
    let decomposed = dir
        .path()
        .join("\u{930}\u{93c}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}");
    write::create_dir_all(&composed).unwrap();
    let apfs = decomposed.symlink_metadata().is_ok();
    assert_eq!(apfs, cfg!(target_os = "macos"), "{}", dir.path().display());
    if !apfs {
        write::create_dir_all(&decomposed).unwrap();
    }
    assert!(same_or_nested(&composed, &decomposed));
    assert!(same_or_nested(&decomposed, &composed));
    assert!(check_folder(&decomposed, std::slice::from_ref(&composed)).is_err());
    assert_eq!(
        same_folder(&composed, &decomposed),
        cfg!(target_os = "macos")
    );
    assert!(same_folder(&composed, &composed));
}

/// A Devanagari name with a nukta letter, `ऱानून`: its composed form (U+0931 …) and its NFD
/// form (U+0930 U+093C …) differ. The file-system fold composes only on macOS, where APFS matches
/// the forms; ext4 and NTFS keep them apart, so the forms stay different there. The safety fold
/// composes on every OS. (`क़` is not used: U+0958 is a composition exclusion, so its NFC form is
/// the decomposed one.)
#[test]
fn a_decomposed_name_folds_to_its_composed_form_as_each_file_system_compares() {
    let spelled = "\u{931}\u{93e}\u{928}\u{942}\u{928}";
    let composed = OsString::from(spelled.nfc().collect::<String>());
    let decomposed = OsString::from(spelled.nfd().collect::<String>());
    assert_ne!(decomposed, composed);
    assert!(
        composed
            .to_str()
            .is_some_and(|text| text.starts_with('\u{931}'))
    );
    assert_eq!(
        fold(decomposed.clone(), Fold::Safety),
        fold(composed.clone(), Fold::Safety)
    );
    assert_eq!(
        fold(decomposed, Fold::FileSystem) == fold(composed, Fold::FileSystem),
        cfg!(target_os = "macos")
    );
}

/// A network share's root is a whole volume, as a drive is: refused in both spellings, with or
/// without the trailing separator, and so is the admin share; a folder inside a share is not
/// (clauseo-ai/clauseo-acp#244).
#[cfg(windows)]
#[test]
fn share_roots_are_drive_roots_on_windows() {
    use super::is_drive_root;
    for root in [
        r"\\server\share\",
        r"\\server\share",
        r"\\?\UNC\server\share\",
        r"\\?\UNC\server\share",
        r"\\localhost\C$\",
        r"\\?\UNC\localhost\C$\",
        r"C:\",
        r"\\?\C:\",
    ] {
        assert!(is_drive_root(Path::new(root)), "{root} is a drive root");
    }
    for inside in [
        r"\\server\share\Clients\X",
        r"\\?\UNC\server\share\Clients",
        r"\\localhost\C$\Users",
        r"C:\Users",
    ] {
        assert!(
            !is_drive_root(Path::new(inside)),
            "{inside} is not a drive root"
        );
    }
}
