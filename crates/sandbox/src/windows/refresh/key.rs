//! The updater public key the admin copy's refresh trusts, and the release guard (ADR 0049,
//! "Admin copy refresh"). `build.rs` includes this file with `#[path]`, so it uses std alone and
//! builds on the host; the crate compiles it too, for its tests and for the refresh's own check.
//!
//! A key here is Tauri's form of a minisign public key: base64 of the whole `.pub` file, an
//! `untrusted comment:` line, then base64 of `Ed`, the 8-byte key id and the 32-byte Ed25519 key.
//! minisign prints the id as the little-endian `u64` in hex (`B142942ACEC5F521`); so do we.

/// The id of the placeholder key: what a build embeds when it is given no updater key (a dry
/// build). Its Ed25519 key is all zeros, so it verifies nothing, and the refresh refuses it by
/// its id before trying, so a dry build's refresh fails closed.
pub const PLACEHOLDER_KEY_ID: u64 = 0xAC98_6E7C_6BA6_7AB9;

/// The id of the throwaway test key a test release signs with
/// (`crates/desktop/release/updater-test.pub`). A release build must not embed it.
pub const TEST_KEY_ID: u64 = 0xB142_942A_CEC5_F521;

/// The placeholder key in Tauri's form: the minisign file `untrusted comment: minisign public
/// key: AC986E7C6BA67AB9`, then `RWS5eqZrfG6YrAAA...`, whose key bytes are all zeros.
pub const PLACEHOLDER_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFDOTg2\
    RTdDNkJBNjdBQjkKUldTNWVxWnJmRzZZckFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUEK";

/// `key` with every ASCII space and line break taken out: a key from the environment or a
/// config file may carry a trailing line break, which isn't part of the base64.
pub fn compact(key: &str) -> String {
    key.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

/// The id of the key `tauri_key` (Tauri's base64 of a minisign public-key file), or why it isn't
/// one: not base64, not UTF-8, no second line, or a second line that isn't 42 bytes starting
/// `Ed`.
pub fn key_id(tauri_key: &str) -> Result<u64, String> {
    let file = decode_base64(&compact(tauri_key)).ok_or("the key isn't base64")?;
    let file =
        String::from_utf8(file).map_err(|error| format!("the key's file isn't UTF-8: {error}"))?;
    let line = file
        .lines()
        .nth(1)
        .ok_or("the key's file has no key line")?;
    let bytes = decode_base64(line.trim()).ok_or("the key line isn't base64")?;
    match bytes.as_slice() {
        [b'E', b'd', id @ ..] if id.len() == 40 => {
            let mut le = [0u8; 8];
            le.copy_from_slice(id.get(..8).ok_or("the key line is short")?);
            Ok(u64::from_le_bytes(le))
        }
        _ => Err("the key line isn't a minisign Ed25519 public key".to_owned()),
    }
}

/// The release guard: `Err` when a release build would embed a key that can't sign a real
/// update. A build is a release when cargo's profile is `release` and `CLAUSEO_RELEASE_MODE` is
/// `release`; such a build must not embed [`PLACEHOLDER_KEY_ID`] or [`TEST_KEY_ID`]. Any other
/// build (debug, a dry or test run) may.
pub fn release_guard(release_mode: Option<&str>, profile: &str, key_id: u64) -> Result<(), String> {
    if release_mode != Some("release") || profile != "release" {
        return Ok(());
    }
    let named = match key_id {
        PLACEHOLDER_KEY_ID => "the placeholder key",
        TEST_KEY_ID => "the throwaway test key",
        _ => return Ok(()),
    };
    Err(format!(
        "a release build (CLAUSEO_RELEASE_MODE=release) would embed {named} ({key_id:016X}) as \
         the updater key the sandbox's admin copy trusts: set CLAUSEO_UPDATER_PUBKEY or \
         plugins.updater.pubkey in crates/desktop/tauri.conf.json"
    ))
}

/// Standard base64 (RFC 4648, `+/`, `=` padding) decoded; `None` for any other character, a
/// misplaced `=` or a length that isn't a multiple of 4.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let (quads, rest) = bytes.as_chunks::<4>();
    if !rest.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(quads.len() * 3);
    for (index, quad) in quads.iter().enumerate() {
        let last = index + 1 == quads.len();
        let pad = quad.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut word = 0u32;
        for &byte in quad.get(..4 - pad)? {
            word = word << 6 | u32::from(sextet(byte)?);
        }
        word <<= 6 * pad;
        let [_, a, b, c] = word.to_be_bytes();
        out.extend_from_slice([a, b, c].get(..3 - pad)?);
    }
    Some(out)
}

/// One base64 character's value.
fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
