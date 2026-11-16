//! The updater rules of the tauri-config check (decision record 0037).
//!
//! - Endpoints need a public key (`Fix::Pubkey`).
//! - The endpoints are none, or exactly [`RELEASE_ENDPOINT`]: the endpoint is decided, so a move
//!   is an edit here too, in front of a reviewer (`Fix::Endpoint`).
//! - A public key is Tauri's form of a minisign public key: base64 of the two-line `.pub` text,
//!   whose second line decodes to 42 bytes, `Ed`, the key id (8 bytes, little-endian) and the
//!   Ed25519 key, and whose untrusted comment names that same id (`Fix::PubkeyFormat`). A secret
//!   key pasted by mistake fails here with its own message.
//! - The key is not the throwaway test key ([`TEST_KEY_ID`], recognised by the id read from the
//!   key's bytes; `Fix::PubkeyTest`).
//! - R23 PR 6's placeholder ([`PLACEHOLDER_KEY_ID`]) passes with a note ([`placeholder_note`]):
//!   a static check runs before every push, and a branch that failed it by design could not be
//!   pushed. The gates that keep it from shipping are the desktop test
//!   `the_release_pubkey_is_not_the_placeholder` (red in CI until Rohan's key, so PR 6 cannot
//!   merge) and release.yml's `check` job, which refuses it in a `release` run.
//! - `bundle.createUpdaterArtifacts` is not `"v1Compatible"`, which needs the updater's `zip`
//!   feature that the workspace leaves off.

use super::{At, Fix};
use crate::violation::Violation;
use serde_json::Value;

/// The one endpoint `plugins.updater.endpoints` may name (0037): the latest published,
/// non-prerelease release's `latest.json` on GitHub.
pub(crate) const RELEASE_ENDPOINT: &str =
    "https://github.com/clauseo-ai/clauseo-acp/releases/latest/download/latest.json";

/// The id of R23 PR 6's placeholder key (`crates/desktop/release/updater-placeholder.pub`), whose
/// private half was deleted when it was made. Stage C's sandbox placeholder carries the same id.
pub(crate) const PLACEHOLDER_KEY_ID: &str = "AC986E7C6BA67AB9";

/// The id of the throwaway test key (`crates/desktop/release/updater-test.pub`).
pub(crate) const TEST_KEY_ID: &str = "B142942ACEC5F521";

/// Every updater rule broken by `conf`.
pub(super) fn updater_violations(conf: &Value, at: &At<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    let endpoints = conf
        .pointer("/plugins/updater/endpoints")
        .and_then(Value::as_array)
        .filter(|endpoints| !endpoints.is_empty());
    let pubkey = conf
        .pointer("/plugins/updater/pubkey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty());
    let endpoints_at = ["plugins", "updater", "endpoints"];
    let pubkey_at = ["plugins", "updater", "pubkey"];
    if let Some(endpoints) = endpoints {
        if pubkey.is_none() {
            found.push(at(
                &endpoints_at,
                "plugins.updater has endpoints but no pubkey".to_owned(),
                Fix::Pubkey,
            ));
        }
        if endpoints.len() != 1 || endpoints[0].as_str() != Some(RELEASE_ENDPOINT) {
            found.push(at(
                &endpoints_at,
                format!("plugins.updater.endpoints is not exactly [\"{RELEASE_ENDPOINT}\"]"),
                Fix::Endpoint,
            ));
        }
    }
    if let Some(pubkey) = pubkey {
        match key_id(pubkey) {
            Err(why) => found.push(at(
                &pubkey_at,
                format!("plugins.updater.pubkey is not a minisign public key: {why}"),
                Fix::PubkeyFormat,
            )),
            Ok(id) if id == TEST_KEY_ID => found.push(at(
                &pubkey_at,
                format!("plugins.updater.pubkey is the throwaway test key ({id})"),
                Fix::PubkeyTest,
            )),
            Ok(_) => {}
        }
    }
    if conf.pointer("/bundle/createUpdaterArtifacts") == Some(&Value::from("v1Compatible")) {
        found.push(at(
            &["bundle", "createUpdaterArtifacts"],
            "bundle.createUpdaterArtifacts is \"v1Compatible\"".to_owned(),
            Fix::V1Compatible,
        ));
    }
    found
}

/// The note printed while `plugins.updater.pubkey` is R23 PR 6's placeholder: it passes, since the
/// desktop guard test and release.yml's `check` job are the gates.
pub(super) fn placeholder_note(conf: &Value) -> Option<String> {
    let pubkey = conf.pointer("/plugins/updater/pubkey")?.as_str()?;
    (key_id(pubkey.trim()).ok()? == PLACEHOLDER_KEY_ID).then(|| {
        format!(
            "plugins.updater.pubkey is still R23 PR 6's placeholder key ({PLACEHOLDER_KEY_ID}): \
             the desktop test the_release_pubkey_is_not_the_placeholder and a v* release refuse \
             it until Rohan's key replaces it"
        )
    })
}

/// The minisign key id of `pubkey` (Tauri's form), read from the key's own bytes and checked
/// against the id its untrusted comment names; an error says what is wrong.
pub(crate) fn key_id(pubkey: &str) -> Result<String, String> {
    let text = base64(pubkey).ok_or("it is not base64")?;
    let text = String::from_utf8(text)
        .ok()
        .ok_or("it is not UTF-8 once decoded")?;
    let mut lines = text.lines();
    let comment = lines.next().unwrap_or_default().trim_end();
    if comment.contains("secret key") {
        return Err("it is a SECRET key; remove it from the history and make a new key".to_owned());
    }
    let named = comment
        .strip_prefix("untrusted comment: minisign public key: ")
        .ok_or("its first line is not a minisign public key's comment")?;
    let key = base64(lines.next().unwrap_or_default()).ok_or("its key line is not base64")?;
    if key.len() != 42 || !key.starts_with(b"Ed") {
        return Err(format!(
            "its key line is not an Ed25519 minisign key ({} bytes)",
            key.len()
        ));
    }
    let id_bytes = <[u8; 8]>::try_from(&key[2..10])
        .ok()
        .ok_or("its key line has no key id")?;
    let id = format!("{:016X}", u64::from_le_bytes(id_bytes));
    if named != id {
        return Err(format!(
            "its comment names key id {named}, but the key's own id is {id}"
        ));
    }
    Ok(id)
}

/// Standard base64 with optional padding, decoded; `None` on any other character.
fn base64(text: &str) -> Option<Vec<u8>> {
    let (mut bits, mut held, mut out) = (0u32, 0u32, Vec::new());
    for byte in text.trim().trim_end_matches('=').bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(u8::try_from((bits >> held) & 0xFF).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
pub(super) mod tests;
