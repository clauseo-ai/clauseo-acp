//! Every OS's place rule, on every OS.

use std::path::Path;

use super::{Place, SyncService, mac_place, windows_key, windows_place};

fn synced(service: SyncService) -> Place {
    Place::Synced { service }
}

const HOME: &str = "/Users/ana";

fn mac(folder: &str, local_mount: Option<bool>) -> Place {
    mac_place(Path::new(folder), Some(Path::new(HOME)), local_mount)
}

#[test]
fn mac_cloud_storage_is_synced_with_the_provider_from_its_name() {
    let cloud = "/Users/ana/Library/CloudStorage";
    assert_eq!(
        mac(&format!("{cloud}/OneDrive-Contoso/Clients"), Some(true)),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        mac(&format!("{cloud}/GoogleDrive-ana@example.com"), Some(true)),
        synced(SyncService::GoogleDrive)
    );
    assert_eq!(
        mac(&format!("{cloud}/Dropbox/A"), Some(true)),
        synced(SyncService::Dropbox)
    );
    // A File Provider mount may not say MNT_LOCAL: the synced rule wins.
    assert_eq!(
        mac(&format!("{cloud}/Box-Box/A"), Some(false)),
        synced(SyncService::Box)
    );
    // CloudStorage itself is no provider's folder.
    assert_eq!(mac(cloud, Some(true)), Place::Local);
}

#[test]
fn mac_mobile_documents_is_icloud_drive() {
    assert_eq!(
        mac(
            "/Users/ana/Library/Mobile Documents/com~apple~CloudDocs/Lease",
            Some(true)
        ),
        synced(SyncService::ICloud)
    );
    assert_eq!(
        mac("/Users/ana/Library/Mobile Documents", Some(true)),
        synced(SyncService::ICloud)
    );
}

#[test]
fn mac_no_mnt_local_is_network() {
    assert_eq!(mac("/Volumes/share/Clients", Some(false)), Place::Network);
    assert_eq!(mac("/Volumes/share/Clients", Some(true)), Place::Local);
    // statfs failed: local.
    assert_eq!(mac("/Volumes/share/Clients", None), Place::Local);
}

#[test]
fn mac_cloud_storage_of_another_home_or_no_home_is_not_synced() {
    let folder = "/Users/bo/Library/CloudStorage/OneDrive-Contoso";
    assert_eq!(mac(folder, Some(true)), Place::Local);
    assert_eq!(mac_place(Path::new(folder), None, Some(true)), Place::Local);
    // A sibling whose name only starts like the folder.
    assert_eq!(
        mac("/Users/ana/Library/CloudStorageOld/OneDrive-X", Some(true)),
        Place::Local
    );
}

#[test]
fn mac_a_data_volume_spelling_of_home_or_folder_is_still_synced() {
    let data = "/System/Volumes/Data/Users/ana";
    assert_eq!(
        mac(
            &format!("{data}/Library/CloudStorage/OneDrive-Contoso/Clients"),
            Some(true)
        ),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        mac(&format!("{data}/Library/Mobile Documents/A"), Some(true)),
        synced(SyncService::ICloud)
    );
    // HOME spelt on the data volume, the folder as Finder gives it.
    assert_eq!(
        mac_place(
            Path::new("/Users/ana/Library/CloudStorage/Dropbox/A"),
            Some(Path::new(data)),
            Some(true)
        ),
        synced(SyncService::Dropbox)
    );
    // Another user's folder on the data volume is not ana's.
    assert_eq!(
        mac(
            "/System/Volumes/Data/Users/bo/Library/CloudStorage/OneDrive-X",
            Some(true)
        ),
        Place::Local
    );
}

#[test]
fn mac_a_case_folded_home_is_still_synced() {
    // The service comes from the folder's own spelling.
    assert_eq!(
        mac(
            "/users/ANA/library/cloudstorage/GoogleDrive-ana@example.com/A",
            Some(true)
        ),
        synced(SyncService::GoogleDrive)
    );
    assert_eq!(
        mac("/USERS/ana/LIBRARY/mobile documents/A", Some(true)),
        synced(SyncService::ICloud)
    );
    assert_eq!(
        mac_place(
            Path::new("/Users/ana/Library/CloudStorage/Box-Box"),
            Some(Path::new("/users/Ana")),
            Some(true)
        ),
        synced(SyncService::Box)
    );
}

fn roots() -> Vec<String> {
    vec![
        r"C:\Users\ana\OneDrive".to_owned(),
        r"C:\Users\ana\OneDrive - Contoso\".to_owned(),
    ]
}

#[test]
fn windows_onedrive_folders_are_synced_without_case() {
    assert_eq!(
        windows_place(r"C:\Users\ana\OneDrive\Clients", &roots(), false),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        windows_place(r"c:\users\ANA\onedrive - contoso\Leases", &roots(), false),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        windows_place(r"C:\Users\ana\OneDrive", &roots(), false),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        windows_place(r"\\?\C:\Users\ana\OneDrive\A", &roots(), false),
        synced(SyncService::OneDrive)
    );
    assert_eq!(
        windows_place("C:/Users/ana/OneDrive/A", &roots(), false),
        synced(SyncService::OneDrive)
    );
}

#[test]
fn windows_a_sibling_of_onedrive_is_not_synced() {
    assert_eq!(
        windows_place(r"C:\Users\ana\OneDriveOld\A", &roots(), false),
        Place::Local
    );
    assert_eq!(
        windows_place(r"C:\Users\ana\Documents", &roots(), false),
        Place::Local
    );
    // An empty variable matches nothing.
    assert_eq!(
        windows_place(r"C:\Users\ana", &[String::new()], false),
        Place::Local
    );
}

#[test]
fn windows_remote_drive_or_unc_is_network() {
    assert_eq!(windows_place(r"Z:\Clients", &roots(), true), Place::Network);
    assert_eq!(
        windows_place(r"\\server\share\Clients", &[], false),
        Place::Network
    );
    assert_eq!(
        windows_place(r"\\?\UNC\server\share\Clients", &[], false),
        Place::Network
    );
    assert_eq!(windows_place(r"D:\Clients", &[], false), Place::Local);
}

#[test]
fn windows_onedrive_on_a_share_is_synced() {
    let roots = [r"\\server\home\ana\OneDrive".to_owned()];
    assert_eq!(
        windows_place(r"\\server\home\ana\OneDrive\A", &roots, true),
        synced(SyncService::OneDrive)
    );
}

#[test]
fn windows_keys_fold_the_verbatim_forms() {
    assert_eq!(
        windows_key(r"\\?\UNC\Server\Share\Clients\"),
        r"\\server\share\clients"
    );
    assert_eq!(windows_key(r"\\?\unc\server\share"), r"\\server\share");
    assert_eq!(windows_key(r"\\?\C:\Users\Ana\"), r"c:\users\ana");
    assert_eq!(windows_key("C:/Users/Ana"), r"c:\users\ana");
    // Other verbatim and device forms stay as they are.
    assert_eq!(windows_key(r"\\?\Volume{1234}\A"), r"\\?\volume{1234}\a");
    assert_eq!(windows_key(r"\\.\pipe\x"), r"\\.\pipe\x");
}

#[test]
fn windows_onedrive_matches_across_the_verbatim_forms() {
    let share = [r"\\server\home\ana\OneDrive".to_owned()];
    assert_eq!(
        windows_place(r"\\?\UNC\server\home\ana\OneDrive\A", &share, true),
        synced(SyncService::OneDrive)
    );
    let verbatim_share = [r"\\?\UNC\server\home\ana\OneDrive".to_owned()];
    assert_eq!(
        windows_place(r"\\server\home\ana\OneDrive\A", &verbatim_share, true),
        synced(SyncService::OneDrive)
    );
    let verbatim_disk = [r"\\?\C:\Users\ana\OneDrive".to_owned()];
    assert_eq!(
        windows_place(r"C:\Users\ana\OneDrive\A", &verbatim_disk, false),
        synced(SyncService::OneDrive)
    );
}

#[test]
fn service_is_the_name_before_its_first_dash() {
    let cases = [
        ("OneDrive-Personal", SyncService::OneDrive),
        ("OneDrive - Contoso Ltd", SyncService::OneDrive),
        ("Dropbox", SyncService::Dropbox),
        ("-odd", SyncService::Other),
    ];
    for (name, service) in cases {
        assert_eq!(SyncService::from_folder_name(name), service, "{name}");
    }
}

#[test]
fn mac_cloud_storage_of_an_unknown_provider_is_synced_by_another_service() {
    assert_eq!(
        mac("/Users/ana/Library/CloudStorage/pCloud-ana/Leases", None),
        synced(SyncService::Other)
    );
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_is_always_local() {
    assert_eq!(super::place(Path::new("/mnt/nfs/share")), Place::Local);
}
