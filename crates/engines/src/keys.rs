//! The updater public key a list must be signed with, fixed at compile time the way
//! `crates/sandbox/build.rs` fixes the admin copy's: `CLAUSEO_UPDATER_PUBKEY` when the build sets
//! it (release.yml sets it per mode, so a test release trusts the test key), else
//! `plugins.updater.pubkey` from `crates/desktop/tauri.conf.json`. Either is Tauri's form of a
//! minisign public key: base64 of the `.pub` file (`untrusted comment: minisign public key: …`,
//! then the `RW…` key line). ASCII whitespace is taken out first, so a trailing line break in the
//! variable doesn't count; an empty value counts as unset.
//!
//! This crate has no build script (it's held to no I/O, and core-io bans build scripts there),
//! so the sources are compiled in directly: the variables through `option_env!`, which cargo
//! tracks, and the files through `include_str!`. With no key, [`load`] is
//! [`Error::NoKey`](crate::Error::NoKey); there is no placeholder.
//!
//! The release guard is a const assertion, so a bad release build fails to compile: with
//! `CLAUSEO_RELEASE_MODE=release`, `CLAUSEO_UPDATER_PUBKEY` must be set (release.yml sets it to
//! `plugins.updater.pubkey` in a release) and its key id must be the one
//! `crates/desktop/release/updater-release.keyid` names, the file release.yml's own guard and
//! the desktop's release-key test read. A `CLAUSEO_RELEASE_MODE` other than `release`, `test`
//! or `dry` (the empty string included) fails too, as the sandbox's guard does, so a typo can't
//! turn it off. Other modes and local builds trust whichever key they were given.
//!
//! [`load`]: crate::load

/// `CLAUSEO_UPDATER_PUBKEY` as the build saw it.
pub const UPDATER_KEY_FROM_ENV: Option<&str> = option_env!("CLAUSEO_UPDATER_PUBKEY");

/// `crates/desktop/tauri.conf.json`, as the build saw it.
pub const TAURI_CONF: &str = include_str!("../../desktop/tauri.conf.json");

/// `CLAUSEO_RELEASE_MODE` as the build saw it: release.yml's `release`, `test` or `dry`.
pub const RELEASE_MODE: Option<&str> = option_env!("CLAUSEO_RELEASE_MODE");

/// The release key's id, from `crates/desktop/release/updater-release.keyid` (16 upper-case
/// hex digits, minisign's form of the little-endian id).
pub const RELEASE_KEY_ID: u64 =
    match hex_id(include_str!("../../desktop/release/updater-release.keyid")) {
        Some(id) => id,
        None => panic!("crates/desktop/release/updater-release.keyid is not 16 hex digits"),
    };

// The guard, at compile time: a release build with the wrong key does not compile.
const _: () = if let Err(why) = release_guard(RELEASE_MODE, UPDATER_KEY_FROM_ENV) {
    panic!("{}", why);
};

/// The updater key lists are checked against, in Tauri's form with no whitespace: the
/// variable's when non-empty, else the config's `plugins.updater.pubkey`; `None` with neither.
pub fn updater_key() -> Option<String> {
    let from_env = UPDATER_KEY_FROM_ENV.map(compact).unwrap_or_default();
    if !from_env.is_empty() {
        return Some(from_env);
    }
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).ok()?;
    let from_conf = compact(conf.pointer("/plugins/updater/pubkey")?.as_str()?);
    (!from_conf.is_empty()).then_some(from_conf)
}

/// `text` with every ASCII space and line break taken out.
pub(crate) fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

/// The guard in the module docs for `mode` (`CLAUSEO_RELEASE_MODE`) and `key`
/// (`CLAUSEO_UPDATER_PUBKEY`): why the build must fail, if it must. A `const fn`, which the
/// crate evaluates at compile time on what the build saw.
pub const fn release_guard(mode: Option<&str>, key: Option<&str>) -> Result<(), &'static str> {
    let Some(mode) = mode else {
        return Ok(());
    };
    if eq(mode, "test") || eq(mode, "dry") {
        return Ok(());
    }
    if !eq(mode, "release") {
        return Err("CLAUSEO_RELEASE_MODE is not release, test, dry or unset");
    }
    let Some(key) = key else {
        return Err("a release build (CLAUSEO_RELEASE_MODE=release) needs CLAUSEO_UPDATER_PUBKEY");
    };
    match key_id(key) {
        Some(id) if id == RELEASE_KEY_ID => Ok(()),
        Some(_) => Err("CLAUSEO_UPDATER_PUBKEY is not the release key that \
             crates/desktop/release/updater-release.keyid names"),
        None => Err("CLAUSEO_UPDATER_PUBKEY is not Tauri's form of a minisign public key"),
    }
}

/// The key id of `tauri_key` (Tauri's base64 of a minisign `.pub` file, ASCII whitespace
/// ignored): the 8 bytes after `Ed` on the file's second line, little-endian. `None` when it
/// isn't such a key. A `const fn`, for the release guard.
#[expect(
    clippy::indexing_slicing,
    reason = "const fn: slice::get isn't const; every index is checked against the length first"
)]
pub const fn key_id(tauri_key: &str) -> Option<u64> {
    let mut file = [0u8; 512];
    let Some(len) = decode(tauri_key.as_bytes(), 0, tauri_key.len(), &mut file) else {
        return None;
    };
    // The key line: from after the first line break to the next one (or the end), less a `\r`.
    let mut start = 0;
    while start < len && file[start] != b'\n' {
        start += 1;
    }
    start += 1;
    let mut end = start;
    while end < len && file[end] != b'\n' {
        end += 1;
    }
    if end > len {
        return None;
    }
    if end > start && file[end - 1] == b'\r' {
        end -= 1;
    }
    let (line, _) = file.split_at(end);
    let mut key = [0u8; 64];
    match decode(line, start, end, &mut key) {
        Some(42) if key[0] == b'E' && key[1] == b'd' => {}
        _ => return None,
    }
    let mut id = 0u64;
    let mut at = 9;
    while at >= 2 {
        id = id << 8 | key[at] as u64;
        at -= 1;
    }
    Some(id)
}

/// Standard base64 (`+/`, `=` padding) of `text[from..to]` decoded into `out`, ASCII whitespace
/// skipped; the byte count, or `None` for another character, misplaced padding or no room.
#[expect(
    clippy::indexing_slicing,
    reason = "const fn: slice::get isn't const; every index is checked against the length first"
)]
const fn decode(text: &[u8], from: usize, to: usize, out: &mut [u8]) -> Option<usize> {
    let (mut word, mut bits, mut len, mut padding) = (0u32, 0u32, 0usize, 0usize);
    let mut at = from;
    while at < to {
        let byte = text[at];
        at += 1;
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        let Some(value) = sextet(byte) else {
            return None;
        };
        word = (word << 6 | value as u32) & 0xFFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if len >= out.len() {
                return None;
            }
            // The low byte: `word` holds at most 16 bits, the top `bits` of them already used.
            let [low, ..] = (word >> bits).to_le_bytes();
            out[len] = low;
            len += 1;
        }
    }
    if padding > 2 {
        return None;
    }
    Some(len)
}

/// One base64 character's value.
const fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// 16 hex digits (surrounding ASCII whitespace ignored) as a `u64`.
#[expect(
    clippy::indexing_slicing,
    reason = "const fn: slice::get isn't const; every index is checked against the length first"
)]
const fn hex_id(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let (mut id, mut digits, mut at) = (0u64, 0, 0);
    while at < bytes.len() {
        let byte = bytes[at];
        at += 1;
        let value = match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            _ if byte.is_ascii_whitespace() => continue,
            _ => return None,
        };
        id = id << 4 | value as u64;
        digits += 1;
    }
    if digits == 16 { Some(id) } else { None }
}

/// `a == b`, in a const context.
#[expect(
    clippy::indexing_slicing,
    reason = "const fn: slice::get isn't const; every index is checked against the length first"
)]
const fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut at = 0;
    while at < a.len() {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}
