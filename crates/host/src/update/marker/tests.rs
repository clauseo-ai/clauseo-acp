use std::path::PathBuf;

use clauseo_settings::Locations;

use super::{clear_marker, find_cached, read_update_marker, write_marker};
use crate::update::{CachedDownload, MarkerState, UpdateMarker};
use crate::write;

/// A fresh data folder under the temp folder for `test`.
fn locations(test: &str) -> (PathBuf, Locations) {
    let root = std::env::temp_dir().join(format!("clauseo-marker-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    let locations = Locations::under_root(&root);
    (root, locations)
}

fn marker() -> UpdateMarker {
    UpdateMarker {
        from: "0.4.0".to_owned(),
        to: "0.4.1".to_owned(),
        state: MarkerState::Installing,
    }
}

#[test]
fn a_written_marker_reads_back_and_clears_once() {
    let (root, locations) = locations("round_trip");
    assert_eq!(read_update_marker(&locations), None);
    write_marker(&locations, &marker()).unwrap();
    let text = std::fs::read_to_string(locations.update_marker()).unwrap();
    assert_eq!(
        text,
        r#"{"from":"0.4.0","to":"0.4.1","state":"installing"}"#
    );
    assert_eq!(read_update_marker(&locations), Some(marker()));
    clear_marker(&locations).unwrap();
    assert_eq!(read_update_marker(&locations), None);
    // Already gone: still fine.
    clear_marker(&locations).unwrap();
    // No temporary file is left beside it.
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn an_empty_or_garbled_marker_reads_as_none() {
    let (root, locations) = locations("garbled");
    for text in [
        "",
        "  \n",
        "{\"from\":\"0.4.0\"",
        "not json",
        "{\"from\":1,\"to\":2}",
    ] {
        write::write_file(&locations.update_marker(), text.as_bytes()).unwrap();
        assert_eq!(read_update_marker(&locations), None, "{text:?}");
    }
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn the_cached_download_is_the_newest_folder_with_one_file() {
    let (root, locations) = locations("cached");
    let updates = locations.updates_dir();
    assert_eq!(find_cached(&locations, "0.4.0"), None);
    let folder = |version: &str, files: &[&str]| {
        let at = updates.join(version);
        write::create_dir_all(&at).unwrap();
        for file in files {
            write::write_file(&at.join(file), b"x").unwrap();
        }
        at
    };
    folder("0.4.0", &["Clauseo.exe"]);
    let nine = folder("0.4.9", &["Clauseo.exe"]);
    folder("0.4.10", &["a.exe", "b.exe"]);
    folder("0.4.11", &[]);
    folder("notes", &["x"]);
    assert_eq!(
        find_cached(&locations, "0.4.0"),
        Some(CachedDownload {
            version: "0.4.9".to_owned(),
            path: nine.join("Clauseo.exe"),
        })
    );
    // The running version's own folder, and older ones, are never offered.
    assert_eq!(find_cached(&locations, "0.4.9"), None);
    write::remove_dir_all(&root).unwrap();
}
