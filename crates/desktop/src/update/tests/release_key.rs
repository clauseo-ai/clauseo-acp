//! The release key (R23 PR 6): `tauri.conf.json`'s `plugins.updater.pubkey` must be Rohan's
//! public key, never the placeholder PR 6 was built against.
//!
//! The placeholder is a throwaway minisign key made for PR 6 alone; its private half and password
//! were deleted at once, so nothing can sign for it. Its public half is committed beside the test
//! key until Rohan's key replaces it. The swap: paste his `.pub` text into `tauri.conf.json`,
//! delete the placeholder's `.pub` file and
//! [`the_committed_placeholder_has_the_key_id_the_guard_refuses`]. The guard stays: it refuses the
//! placeholder by its key id, held here, so the placeholder can never come back. The key id is
//! read from the key's own bytes and must match the comment's ([`key_id`]). `release.yml`'s
//! check job refuses the same keys in a release run, which is the control a merge cannot skip.

use serde_json::Value;

use super::test_key::{base64, key_id};
use super::{crate_dir, test_key_file};

/// The placeholder's minisign key id (from its key bytes; its comment names the same).
const PLACEHOLDER_KEY_ID: &str = "AC986E7C6BA67AB9";

/// The platform configs Tauri merges over `tauri.conf.json` on its own (none yet).
const PLATFORM_CONFIGS: [&str; 3] = [
    "tauri.macos.conf.json",
    "tauri.windows.conf.json",
    "tauri.linux.conf.json",
];

/// `plugins.updater.pubkey` of `tauri.conf.json`, of each platform config that exists and of
/// each release overlay, where one sets it.
fn release_pubkeys() -> Vec<(String, String)> {
    let mut configs = vec![crate_dir().join("tauri.conf.json")];
    configs.extend(
        PLATFORM_CONFIGS
            .iter()
            .map(|name| crate_dir().join(name))
            .filter(|path| path.is_file()),
    );
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
        let id = key_id(&pubkey)
            .unwrap_or_else(|why| panic!("{config}: not a minisign public key: {why}"));
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
    assert_eq!(key_id(&placeholder).as_deref(), Ok(PLACEHOLDER_KEY_ID));
}

/// Standard base64 with padding, for the doctored keys below.
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (at, byte)| {
            n | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..4 {
            if at <= chunk.len() {
                let index = usize::try_from((n >> (18 - 6 * at)) & 63).unwrap();
                out.push(char::from(ALPHABET[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The placeholder's `.pub` text with its comment's id and its key line's bytes changed by
/// `comment` and `key`, encoded again as `tauri signer` writes it. Built from the placeholder's
/// bytes; no key pair is made.
fn doctored(comment: impl Fn(&str) -> String, key: impl Fn(Vec<u8>) -> Vec<u8>) -> String {
    let placeholder =
        std::fs::read_to_string(crate_dir().join("release/updater-placeholder.pub")).unwrap();
    let text = String::from_utf8(base64(&placeholder)).unwrap();
    let mut lines = text.lines();
    let first = comment(lines.next().unwrap());
    let second = encode(&key(base64(lines.next().unwrap())));
    encode(format!("{first}\n{second}\n").as_bytes())
}

/// The id comes from the key's bytes: a comment naming another id is refused, and so is a key
/// line that is not 42 bytes starting `Ed`. Unchanged, the doctoring keeps the key as it was.
#[test]
fn the_key_id_is_read_from_the_key_and_must_match_its_comment() {
    let same = doctored(str::to_owned, |key| key);
    assert_eq!(key_id(&same).as_deref(), Ok(PLACEHOLDER_KEY_ID));

    let renamed = doctored(
        |line| line.replace(PLACEHOLDER_KEY_ID, "0123456789ABCDEF"),
        |key| key,
    );
    let why = key_id(&renamed).unwrap_err();
    assert!(
        why.contains(PLACEHOLDER_KEY_ID) && why.contains("0123456789ABCDEF"),
        "{why}"
    );

    let short = doctored(str::to_owned, |mut key| {
        key.pop();
        key
    });
    assert!(key_id(&short).unwrap_err().contains("41 bytes"));
    let not_ed = doctored(str::to_owned, |mut key| {
        key[1] = b'X';
        key
    });
    assert!(key_id(&not_ed).unwrap_err().contains("not an Ed25519"));
}
