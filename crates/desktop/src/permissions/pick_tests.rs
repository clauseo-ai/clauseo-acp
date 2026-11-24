//! The pick-time permissions (`pick.rs`): where a picked folder is, from its path and the mount
//! table, and the ask that lists it, over the fake [`Probe`] of `tests.rs`. The macOS paths are
//! only compared, never opened.
// unix-paths-ok: the macOS paths are only compared; no test opens one

use std::cell::Cell;
use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, FolderRefId};
use serde_json::json;
use uuid::Uuid;

use super::pick::{Mount, ask_at_pick, at_pick, classify, parse_mounts, picked_folder};
use super::tests::{Fake, ids, store};
use super::*;

/// The user's home folder, as the pick gives it.
const HOME: &str = "/Users/ada";

fn mount(point: &str, fs_type: &str) -> Mount {
    Mount {
        point: PathBuf::from(point),
        fs_type: fs_type.to_owned(),
    }
}

/// The system's mounts: the root and the data volume that holds home.
fn system_mounts() -> Vec<Mount> {
    vec![mount("/", "apfs"), mount("/System/Volumes/Data", "apfs")]
}

/// What `folder` needs, with home at [`HOME`] and `mounts` as the mount table.
fn at(folder: &str, mounts: Vec<Mount>) -> Option<PermissionId> {
    classify(Path::new(folder), Some(Path::new(HOME)), || mounts)
}

#[test]
fn the_desktop_and_downloads_are_by_name_the_folder_itself_or_inside() {
    assert_eq!(
        at("/Users/ada/Desktop", system_mounts()),
        Some(PermissionId::FilesDesktop)
    );
    assert_eq!(
        at("/Users/ada/Desktop/Acme", system_mounts()),
        Some(PermissionId::FilesDesktop)
    );
    assert_eq!(
        at("/Users/ada/Downloads/Acme/2026", system_mounts()),
        Some(PermissionId::FilesDownloads)
    );
}

#[test]
fn the_cloud_folders_are_by_name_under_library() {
    for folder in [
        "/Users/ada/Library/CloudStorage/OneDrive-Acme/Matters",
        "/Users/ada/Library/CloudStorage",
        "/Users/ada/Library/Mobile Documents/com~apple~CloudDocs/Acme",
    ] {
        assert_eq!(
            at(folder, system_mounts()),
            Some(PermissionId::FilesCloud),
            "{folder}"
        );
    }
}

#[test]
fn a_folder_named_like_one_of_them_is_none() {
    for folder in [
        "/Users/ada/Desktop2/Acme",
        "/Users/ada/Downloads-old/Acme",
        "/Users/ada/Library/CloudStorage2/Acme",
        "/Users/ada/Library/Mobile Documents Old/Acme",
    ] {
        assert_eq!(at(folder, system_mounts()), None, "{folder}");
    }
}

#[test]
fn documents_and_other_folders_on_the_system_volume_are_none() {
    for folder in [
        "/Users/ada/Documents/Acme",
        "/Users/ada/Projects/Acme",
        "/Users/ada",
        "/System/Volumes/Data/Acme",
    ] {
        assert_eq!(at(folder, system_mounts()), None, "{folder}");
    }
}

#[test]
fn the_root_and_the_data_volume_are_none_whatever_their_type() {
    // The system's own mounts never count as a volume, even read as a network type.
    let odd = vec![mount("/", "smbfs"), mount("/System/Volumes/Data", "nfs")];
    assert_eq!(at("/Volumes/Acme", odd), None);
}

#[test]
fn the_mount_table_is_read_only_for_a_folder_under_volumes() {
    let read = Cell::new(false);
    let mounts = || {
        read.set(true);
        system_mounts()
    };
    let home = Some(Path::new(HOME));
    assert_eq!(
        classify(Path::new("/Users/ada/Desktop/Acme"), home, mounts),
        Some(PermissionId::FilesDesktop)
    );
    assert!(!read.get(), "a Desktop folder needs no mount table");
    assert_eq!(
        classify(Path::new("/Users/ada/Documents/Acme"), home, mounts),
        None
    );
    assert!(!read.get(), "Documents needs no mount table");
    assert_eq!(classify(Path::new("/Volumes/USB/Acme"), home, mounts), None);
    assert!(read.get(), "a folder under /Volumes reads it");
}

#[test]
fn a_network_volume_is_files_network_whatever_its_type_name() {
    for fs_type in ["smbfs", "cifs", "afpfs", "nfs", "webdav"] {
        let mut mounts = system_mounts();
        mounts.push(mount("/Volumes/Office", fs_type));
        assert_eq!(
            at("/Volumes/Office/Acme", mounts),
            Some(PermissionId::FilesNetwork),
            "{fs_type}"
        );
    }
}

#[test]
fn a_mount_outside_volumes_is_not_read_so_it_is_none() {
    let mut mounts = system_mounts();
    mounts.push(mount("/Users/ada/mnt", "nfs"));
    assert_eq!(at("/Users/ada/mnt/Acme", mounts), None);
}

#[test]
fn another_volume_under_volumes_is_files_removable() {
    let mut mounts = system_mounts();
    mounts.push(mount("/Volumes/USB", "msdos"));
    mounts.push(mount("/Volumes/Work Drive", "apfs"));
    assert_eq!(
        at("/Volumes/USB/Acme", mounts.clone()),
        Some(PermissionId::FilesRemovable)
    );
    assert_eq!(
        at("/Volumes/Work Drive/Acme", mounts),
        Some(PermissionId::FilesRemovable)
    );
}

#[test]
fn the_longest_mount_that_holds_the_folder_decides() {
    let mut mounts = system_mounts();
    mounts.push(mount("/Volumes/Work", "msdos"));
    mounts.push(mount("/Volumes/Work/Share", "smbfs"));
    assert_eq!(
        at("/Volumes/Work/Share/Acme", mounts.clone()),
        Some(PermissionId::FilesNetwork)
    );
    assert_eq!(
        at("/Volumes/Work/Acme", mounts),
        Some(PermissionId::FilesRemovable)
    );
}

#[test]
fn a_folder_named_like_volumes_is_not_under_it() {
    let mut mounts = system_mounts();
    mounts.push(mount("/Volumes2", "msdos"));
    assert_eq!(at("/Volumes2/Acme", mounts), None);
}

#[test]
fn without_a_home_only_the_mount_table_decides() {
    let mut mounts = system_mounts();
    mounts.push(mount("/Volumes/USB", "msdos"));
    assert_eq!(
        classify(Path::new("/Users/ada/Desktop"), None, || mounts.clone()),
        None
    );
    assert_eq!(
        classify(Path::new("/Volumes/USB/Acme"), None, || mounts),
        Some(PermissionId::FilesRemovable)
    );
}

#[test]
fn off_macos_nothing_is_asked_at_a_pick() {
    let mut windows = Fake::windows(true);
    windows.mounts = vec![mount("/Volumes/Office", "smbfs")];
    let mut other = Fake::new(Platform::Other);
    other.mounts = vec![mount("/Volumes/Office", "smbfs")];
    for probe in [windows, other] {
        assert_eq!(
            at_pick(
                &probe,
                Path::new("C:/Users/ada/Desktop/Acme"),
                Some(Path::new("C:/Users/ada"))
            ),
            None
        );
    }
}

#[test]
fn on_macos_a_pick_reads_the_mount_table_and_lists_nothing() {
    let mut probe = Fake::new(Platform::Mac);
    probe.mounts = vec![mount("/", "apfs"), mount("/Volumes/USB", "msdos")];
    let home = Some(Path::new(HOME));
    assert_eq!(
        at_pick(&probe, Path::new("/Volumes/USB/Acme"), home),
        Some(PermissionId::FilesRemovable)
    );
    assert_eq!(
        at_pick(&probe, Path::new("/Users/ada/Documents/Acme"), home),
        None
    );
    assert_eq!(probe.asked(), Vec::<String>::new());
}

#[test]
fn the_pick_ids_are_pick_time_and_the_others_are_not() {
    for id in [
        PermissionId::FilesDesktop,
        PermissionId::FilesDownloads,
        PermissionId::FilesRemovable,
        PermissionId::FilesNetwork,
        PermissionId::FilesCloud,
    ] {
        assert!(id.is_pick_time(), "{id:?}");
    }
    for id in [
        PermissionId::FilesDocuments,
        PermissionId::Keychain,
        PermissionId::ControlledFolderAccess,
    ] {
        assert!(!id.is_pick_time(), "{id:?}");
    }
}

#[test]
fn the_pick_ids_are_never_listed() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut probe = Fake::new(Platform::Mac);
    probe.mounts = vec![mount("/", "apfs"), mount("/Volumes/USB", "msdos")];
    let listed = list(&probe, &store.asked()).rows;
    assert!(
        listed.iter().all(|row| !row.id.is_pick_time()),
        "{listed:?}"
    );
    // The matters folder is in Documents, so only that one is listed.
    assert_eq!(ids(&listed), vec![PermissionId::FilesDocuments]);
}

#[test]
fn the_mount_listing_parses_each_line_and_skips_the_rest() {
    let listing = "\
/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)
/dev/disk3s5 on /System/Volumes/Data (apfs, local, journaled, protect, nobrowse)
map auto_home on /System/Volumes/Data/home (autofs, automounted, nobrowse)
//rohan@nas.local/Matters on /Volumes/Matters (smbfs, nodev, nosuid, mounted by rohan)
/dev/disk4s1 on /Volumes/USB Stick (msdos, local, nodev, nosuid, noowners)
this line has no mount point
/dev/disk5 on /Volumes/Empty ()
/dev/disk6 on  (exfat)
";
    assert_eq!(
        parse_mounts(listing),
        vec![
            mount("/", "apfs"),
            mount("/System/Volumes/Data", "apfs"),
            mount("/System/Volumes/Data/home", "autofs"),
            mount("/Volumes/Matters", "smbfs"),
            mount("/Volumes/USB Stick", "msdos"),
        ]
    );
}

#[test]
fn a_network_share_and_a_stick_with_spaces_classify_from_the_parsed_listing() {
    let mounts = parse_mounts(
        "//rohan@nas.local/Matters on /Volumes/Matters (smbfs, nodev)\n\
         /dev/disk4s1 on /Volumes/USB Stick (msdos, local)\n",
    );
    assert_eq!(
        at("/Volumes/Matters/Acme", mounts.clone()),
        Some(PermissionId::FilesNetwork)
    );
    assert_eq!(
        at("/Volumes/USB Stick/Acme", mounts),
        Some(PermissionId::FilesRemovable)
    );
}

#[test]
fn the_pick_ids_go_on_the_wire_in_snake_case_both_ways() {
    for (id, name) in [
        (PermissionId::FilesDesktop, "files_desktop"),
        (PermissionId::FilesDownloads, "files_downloads"),
        (PermissionId::FilesRemovable, "files_removable"),
        (PermissionId::FilesNetwork, "files_network"),
        (PermissionId::FilesCloud, "files_cloud"),
    ] {
        assert_eq!(serde_json::to_value(id).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<PermissionId>(json!(name)).unwrap(),
            id
        );
    }
}

#[test]
fn a_pick_ask_needs_the_folder_it_was_picked_for() {
    let folder = FolderRefId::new(Uuid::now_v7()).unwrap();
    let error = picked_folder(None, |_| None).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    let error = picked_folder(Some(folder), |_| None).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    let path = PathBuf::from("/Users/ada/Desktop/Acme");
    assert_eq!(
        picked_folder(Some(folder), |asked| (asked == folder)
            .then(|| path.clone()))
        .unwrap(),
        path
    );
}

#[test]
fn asking_for_a_picked_folder_grants_when_listing_it_works() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let probe = Fake::new(Platform::Mac);
    let row = ask_at_pick(
        &probe,
        PermissionId::FilesDesktop,
        Path::new("/Users/ada/Desktop/Acme"),
    )
    .unwrap();
    assert_eq!(
        row,
        PermissionRow {
            id: PermissionId::FilesDesktop,
            state: OsPermissionState::Granted,
            folder: Some("/Users/ada/Desktop/Acme".to_owned()),
            settings_path: None,
            note: None,
            asked: true,
        }
    );
    assert_eq!(probe.asked(), vec!["/Users/ada/Desktop/Acme".to_owned()]);
    assert!(
        store.asked().is_empty(),
        "nothing recorded: macOS remembers"
    );
}

#[test]
fn asking_for_a_picked_folder_is_denied_when_listing_it_fails() {
    let mut probe = Fake::new(Platform::Mac);
    probe.listed = false;
    let row = ask_at_pick(
        &probe,
        PermissionId::FilesCloud,
        Path::new("/Users/ada/Library/CloudStorage/Matters"),
    )
    .unwrap();
    assert_eq!(row.id, PermissionId::FilesCloud);
    assert_eq!(row.state, OsPermissionState::Denied);
    assert_eq!(
        row.folder.as_deref(),
        Some("/Users/ada/Library/CloudStorage/Matters")
    );
    assert_eq!(row.settings_path.as_deref(), Some(FILES_SETTINGS));
}

#[test]
fn asking_for_a_pick_id_off_macos_or_an_old_id_is_refused() {
    let folder = Path::new("/Users/ada/Desktop/Acme");
    let windows = Fake::windows(true);
    let error = ask_at_pick(&windows, PermissionId::FilesDesktop, folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    let mac = Fake::new(Platform::Mac);
    let error = ask_at_pick(&mac, PermissionId::FilesDocuments, folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    assert_eq!(mac.asked(), Vec::<String>::new());
}
