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

/// The production updater key's id, as minisign prints it (16 hex digits): the one key a
/// release build may embed. `None` until #527 adds Rohan's production key; #527 must set it
/// here, in the same change that sets `plugins.updater.pubkey`. Until then every release build
/// fails [`release_guard`], by design.
pub const PRODUCTION_KEY_ID: Option<&str> = None;

/// The name of the `VERSIONINFO` string that carries the embedded key's id (16 hex digits), so
/// a release's assembly can check the key from the exe's file without running it (it asks for an
/// administrator, so running it would prompt).
pub const KEY_ID_VERSION_STRING: &str = "UpdaterKeyId";

/// The values `CLAUSEO_RELEASE_MODE` may take: release.yml's three modes, which its build
/// legs export from `check`'s `mode`. Unset is a local build.
pub const RELEASE_MODES: [&str; 3] = ["release", "test", "dry"];

/// The release guard: in a release build (`CLAUSEO_RELEASE_MODE=release`, in any cargo profile)
/// the embedded key must be [`PRODUCTION_KEY_ID`], which must be set. Any other build (a dry or
/// test run, or a local one) may embed any key; the exe's `UpdaterKeyId` string says which.
/// A mode outside [`RELEASE_MODES`], the empty string included, fails: a typo must not turn the
/// guard off.
pub fn release_guard(release_mode: Option<&str>, key_id: u64) -> Result<(), String> {
    release_guard_with(release_mode, key_id, PRODUCTION_KEY_ID)
}

/// [`release_guard`] with `pinned` as the production key's id.
pub fn release_guard_with(
    release_mode: Option<&str>,
    key_id: u64,
    pinned: Option<&str>,
) -> Result<(), String> {
    let found = format!("{key_id:016X}");
    match release_mode {
        Some("release") => {}
        None => return Ok(()),
        Some(mode) if RELEASE_MODES.contains(&mode) => return Ok(()),
        Some(mode) => {
            return Err(format!(
                "CLAUSEO_RELEASE_MODE is {mode:?}, not one of {RELEASE_MODES:?} or unset; the \
                 embedded updater key is {found}"
            ));
        }
    }
    let Some(pinned) = pinned else {
        return Err(format!(
            "a release build (CLAUSEO_RELEASE_MODE=release) needs PRODUCTION_KEY_ID in \
             crates/sandbox/src/windows/refresh/key.rs, which is unset (#527 sets it); the \
             embedded updater key is {found}"
        ));
    };
    if pinned.eq_ignore_ascii_case(&found) {
        return Ok(());
    }
    let named = match key_id {
        PLACEHOLDER_KEY_ID => "the placeholder key",
        TEST_KEY_ID => "the throwaway test key",
        _ => "a key other than the production one",
    };
    Err(format!(
        "a release build (CLAUSEO_RELEASE_MODE=release) would embed {named} ({found}) as the \
         updater key the sandbox's admin copy trusts, not the production key {pinned}: set \
         CLAUSEO_UPDATER_PUBKEY or plugins.updater.pubkey in crates/desktop/tauri.conf.json"
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
