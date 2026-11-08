//! `SyncService`, `FolderPlace::Synced`'s service: closed, `snake_case`, and named from a synced
//! folder's name without what follows its first `-`.

use clauseo_protocol::SyncService;

#[test]
fn sync_service_is_closed_and_snake_case() {
    let names = [
        "one_drive",
        "dropbox",
        "google_drive",
        "icloud",
        "box",
        "other",
    ];
    for (service, name) in SyncService::ALL.into_iter().zip(names) {
        assert_eq!(
            serde_json::to_value(service).unwrap(),
            serde_json::json!(name)
        );
        let back: SyncService = serde_json::from_value(serde_json::json!(name)).unwrap();
        assert_eq!(back, service);
    }
    // Closed: an old free-text name or a newer value fails to decode.
    for unknown in ["GoogleDrive", "iCloud Drive", "pcloud"] {
        assert!(serde_json::from_value::<SyncService>(serde_json::json!(unknown)).is_err());
    }
}

#[test]
fn sync_service_from_folder_name_keeps_only_the_service() {
    let cases = [
        ("OneDrive-Personal", SyncService::OneDrive),
        ("OneDrive - Contoso Ltd", SyncService::OneDrive),
        ("GoogleDrive-ana@example.com", SyncService::GoogleDrive),
        ("Dropbox", SyncService::Dropbox),
        ("Box-Box", SyncService::Box),
        ("iCloud Drive", SyncService::ICloud),
        ("pCloud-ana", SyncService::Other),
        ("", SyncService::Other),
    ];
    for (name, service) in cases {
        assert_eq!(SyncService::from_folder_name(name), service, "{name}");
    }
}
