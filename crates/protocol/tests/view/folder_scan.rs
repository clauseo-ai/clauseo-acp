//! `FolderScanned`'s wire form: each `FolderPlace` is named (a synced folder's service too), and
//! the last event names no file.

use clauseo_protocol::{Count, FolderPlace, FolderRefId, FolderScanned, HostEvent, SyncService};

use super::fixtures::{round_trips, v7};

#[test]
fn folder_scanned_names_each_place_and_leaves_out_no_file() {
    let scanned = |place: FolderPlace| {
        HostEvent::FolderScanned(FolderScanned {
            folder: FolderRefId::new(v7(5)).unwrap(),
            files: 2,
            bytes: Count::new(10).unwrap(),
            now_reading: None,
            place,
            cloud_only: 0,
            done: true,
            cancelled: true,
        })
    };
    for (place, json) in [
        (FolderPlace::Local, serde_json::json!("local")),
        (FolderPlace::Network, serde_json::json!("network")),
        (
            FolderPlace::Synced {
                service: SyncService::ICloud,
            },
            serde_json::json!({"synced": {"service": "icloud"}}),
        ),
    ] {
        let event = scanned(place);
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["folder_scanned"]["place"], json);
        // The last event names no file.
        assert!(
            value["folder_scanned"].get("now_reading").is_none(),
            "{value}"
        );
        round_trips(&event);
    }
}
