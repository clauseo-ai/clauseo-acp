//! `Info.plist` (beside `tauri.conf.json`, which Tauri merges into the app's): each macOS privacy
//! usage key Clauseo needs is there, with one sentence for all of them. Without a key macOS
//! refuses Clauseo, and every agent it starts, that folder kind in silence, never asking the
//! lawyer (`matter.folder_permission`, `crates/host/src/agent/folder_access.rs`).

/// The keys: Documents, Desktop and Downloads, removable and network volumes, and file
/// providers (iCloud Drive and other synced folders).
const KEYS: [&str; 6] = [
    "NSDocumentsFolderUsageDescription",
    "NSDesktopFolderUsageDescription",
    "NSDownloadsFolderUsageDescription",
    "NSRemovableVolumesUsageDescription",
    "NSNetworkVolumesUsageDescription",
    "NSFileProviderDomainUsageDescription",
];

const PLIST: &str = include_str!("../Info.plist");

/// The `<string>` right after `<key>key</key>`, or `None` when the key is missing or is
/// followed by anything else.
fn string_of(plist: &str, key: &str) -> Option<String> {
    let after = plist
        .split_once(&format!("<key>{key}</key>"))?
        .1
        .trim_start();
    let value = after.strip_prefix("<string>")?;
    Some(value.split_once("</string>")?.0.to_owned())
}

#[test]
fn every_usage_key_is_there_with_one_sentence() {
    let sentences: Vec<String> = KEYS
        .iter()
        .map(|key| string_of(PLIST, key).unwrap_or_else(|| panic!("Info.plist has no {key}")))
        .collect();
    let first = &sentences[0];
    assert!(first.starts_with("Clauseo "), "{first}");
    assert!(first.ends_with('.'), "{first}");
    for (key, sentence) in KEYS.iter().zip(&sentences) {
        assert_eq!(sentence, first, "{key} says something else");
    }
}

#[test]
fn a_missing_key_is_found_missing() {
    let without = PLIST.replace("NSDocumentsFolderUsageDescription", "Other");
    assert_eq!(string_of(&without, KEYS[0]), None);
}
