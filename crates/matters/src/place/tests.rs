//! Every OS's place rule, on every OS.

use std::path::Path;

use super::{Place, mac_place, service_name, windows_place};

fn synced(service: &str) -> Place {
    Place::Synced {
        service: service.to_owned(),
    }
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
        synced("OneDrive")
    );
    assert_eq!(
        mac(&format!("{cloud}/GoogleDrive-ana@example.com"), Some(true)),
        synced("GoogleDrive")
    );
    assert_eq!(
        mac(&format!("{cloud}/Dropbox/A"), Some(true)),
        synced("Dropbox")
    );
    // A File Provider mount may not say MNT_LOCAL: the synced rule wins.
    assert_eq!(
        mac(&format!("{cloud}/Box-Box/A"), Some(false)),
        synced("Box")
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
        synced("iCloud Drive")
    );
    assert_eq!(
        mac("/Users/ana/Library/Mobile Documents", Some(true)),
        synced("iCloud Drive")
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
        synced("OneDrive")
    );
    assert_eq!(
        windows_place(r"c:\users\ANA\onedrive - contoso\Leases", &roots(), false),
        synced("OneDrive")
    );
    assert_eq!(
        windows_place(r"C:\Users\ana\OneDrive", &roots(), false),
        synced("OneDrive")
    );
    assert_eq!(
        windows_place(r"\\?\C:\Users\ana\OneDrive\A", &roots(), false),
        synced("OneDrive")
    );
    assert_eq!(
        windows_place("C:/Users/ana/OneDrive/A", &roots(), false),
        synced("OneDrive")
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
        synced("OneDrive")
    );
}

#[test]
fn service_is_the_name_before_its_first_dash() {
    assert_eq!(service_name("OneDrive-Personal"), "OneDrive");
    assert_eq!(service_name("OneDrive - Contoso Ltd"), "OneDrive");
    assert_eq!(service_name("Dropbox"), "Dropbox");
    assert_eq!(service_name("-odd"), "-odd");
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn linux_is_always_local() {
    assert_eq!(super::place(Path::new("/mnt/nfs/share")), Place::Local);
}
