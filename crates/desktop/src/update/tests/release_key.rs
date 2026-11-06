//! The release key (R23 PR 6): `tauri.conf.json`'s `plugins.updater.pubkey` must be Rohan's
//! public key, never the placeholder PR 6 was built against.
//!
//! The placeholder is a throwaway minisign key made for PR 6 alone; its private half and password
//! were deleted at once, so nothing can sign for it. Its public half is committed beside the test
//! key until Rohan's key replaces it. The swap: paste his `.pub` text into `tauri.conf.json`,
//! delete the placeholder's `.pub` file and
//! [`the_committed_placeholder_has_the_key_id_the_guard_refuses`]. The guard stays: it refuses the
//! placeholder by its key id, held here, so the placeholder can never come back.

use serde_json::Value;

use super::test_key::base64;
use super::{crate_dir, test_key_file};

/// The placeholder's minisign key id (the comment line of its public key).
const PLACEHOLDER_KEY_ID: &str = "AC986E7C6BA67AB9";

/// The minisign key id of a public key as `tauri signer` writes it (base64 of the two-line
/// `.pub` text), or `None` when it is not one.
fn key_id(pubkey: &str) -> Option<String> {
    let text = String::from_utf8(base64(pubkey)).ok()?;
    let comment = text.lines().next()?;
    let id = comment.strip_prefix("untrusted comment: minisign public key: ")?;
    Some(id.trim().to_owned())
}

/// `plugins.updater.pubkey` of `tauri.conf.json` and of each release overlay that sets one.
fn release_pubkeys() -> Vec<(String, String)> {
    let mut configs = vec![crate_dir().join("tauri.conf.json")];
    for entry in std::fs::read_dir(crate_dir().join("release")).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with(".conf.json") {
            configs.push(path);
        }
    }
    configs
        .into_iter()
        .filter_map(|config| {
            let json: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
            let pubkey = json.pointer("/plugins/updater/pubkey")?.as_str()?.trim();
            (!pubkey.is_empty()).then(|| (config.display().to_string(), pubkey.to_owned()))
        })
        .collect()
}

/// The guard: fails while the release key is the placeholder, which keeps PR 6's CI red, so it
/// cannot merge by accident. Rohan's key, once in, passes; it must also be a minisign public key
/// and not the test key.
#[test]
fn the_release_pubkey_is_not_the_placeholder() {
    let pubkeys = release_pubkeys();
    assert!(
        pubkeys
            .iter()
            .any(|(config, _)| config.ends_with("tauri.conf.json")),
        "tauri.conf.json has no plugins.updater.pubkey"
    );
    let test_key_id = key_id(&test_key_file()).unwrap();
    for (config, pubkey) in pubkeys {
        let id = key_id(&pubkey).unwrap_or_else(|| panic!("{config}: not a minisign public key"));
        assert_ne!(
            id, PLACEHOLDER_KEY_ID,
            "{config}: PR 6 can't merge until Rohan's public key replaces the placeholder \
             (R23-key-steps.md)"
        );
        assert_ne!(id, test_key_id, "{config}: the release key is the test key");
    }
}

/// The placeholder's committed public half is the key the guard refuses. Deleted with that file
/// when Rohan's key lands.
#[test]
fn the_committed_placeholder_has_the_key_id_the_guard_refuses() {
    let placeholder =
        std::fs::read_to_string(crate_dir().join("release/updater-placeholder.pub")).unwrap();
    assert_eq!(key_id(&placeholder).as_deref(), Some(PLACEHOLDER_KEY_ID));
}
