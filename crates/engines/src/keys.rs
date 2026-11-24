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
/// isn't such a key. A `const fn`, for the release guard: it decodes both layers of base64 as
/// it goes, keeping no buffer.
pub const fn key_id(tauri_key: &str) -> Option<u64> {
    let mut file = Base64::new(tauri_key.as_bytes());
    // Past the untrusted comment line.
    loop {
        match file.next() {
            Decoded::Byte(b'\n') => break,
            Decoded::Byte(_) => {}
            Decoded::Empty | Decoded::Invalid => return None,
        }
    }
    // The key line, itself base64, up to the next line break or the end.
    let mut line = Base64::new(&[]);
    let (mut count, mut id) = (0u32, 0u64);
    loop {
        let text = match file.next() {
            Decoded::Byte(b'\n' | b'\r') | Decoded::Empty => break,
            Decoded::Byte(text) => text,
            Decoded::Invalid => return None,
        };
        let byte = match line.push(text) {
            Decoded::Byte(byte) => byte,
            Decoded::Empty => continue,
            Decoded::Invalid => return None,
        };
        match count {
            0 if byte == b'E' => {}
            1 if byte == b'd' => {}
            2..=9 => id |= (byte as u64) << (8 * (count - 2)),
            10..=41 => {}
            _ => return None,
        }
        count += 1;
    }
    if count == 42 { Some(id) } else { None }
}

/// What [`Base64`] gives for a step.
#[derive(Clone, Copy)]
enum Decoded {
    /// A decoded byte.
    Byte(u8),
    /// No byte: the end of the text (`next`), or a character that completes none (`push`).
    Empty,
    /// A character that isn't base64, or one after the padding.
    Invalid,
}

/// A standard base64 (`+/`, `=` padding) decoder, a byte at a time, in a const context.
struct Base64<'a> {
    /// The text not read yet.
    rest: &'a [u8],
    /// The bits read and not yet given out, in the low 16.
    word: u32,
    /// How many of `word`'s bits are pending.
    bits: u32,
    /// Whether a `=` was seen: only more `=` and whitespace may follow.
    padded: bool,
}

impl<'a> Base64<'a> {
    const fn new(rest: &'a [u8]) -> Self {
        Self {
            rest,
            word: 0,
            bits: 0,
            padded: false,
        }
    }

    /// The next decoded byte of the text, ASCII whitespace skipped.
    const fn next(&mut self) -> Decoded {
        loop {
            let [text, rest @ ..] = self.rest else {
                return Decoded::Empty;
            };
            self.rest = rest;
            match self.push(*text) {
                Decoded::Empty => {}
                Decoded::Byte(byte) => return Decoded::Byte(byte),
                Decoded::Invalid => return Decoded::Invalid,
            }
        }
    }

    /// Feeds one character (rather than reading `rest`): the byte it completes, if any.
    const fn push(&mut self, text: u8) -> Decoded {
        if text.is_ascii_whitespace() {
            return Decoded::Empty;
        }
        if text == b'=' {
            self.padded = true;
            return Decoded::Empty;
        }
        if self.padded {
            return Decoded::Invalid;
        }
        let Some(value) = sextet(text) else {
            return Decoded::Invalid;
        };
        self.word = (self.word << 6 | value as u32) & 0xFFFF;
        self.bits += 6;
        if self.bits < 8 {
            return Decoded::Empty;
        }
        self.bits -= 8;
        // The low byte: `word` holds at most 16 bits, the top `bits` of them already given out.
        let [low, ..] = (self.word >> self.bits).to_le_bytes();
        Decoded::Byte(low)
    }
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
const fn hex_id(text: &str) -> Option<u64> {
    let mut bytes = text.as_bytes();
    let (mut id, mut digits) = (0u64, 0);
    while let [byte, rest @ ..] = bytes {
        bytes = rest;
        let value = match *byte {
            digit @ b'0'..=b'9' => digit - b'0',
            digit @ b'A'..=b'F' => digit - b'A' + 10,
            other if other.is_ascii_whitespace() => continue,
            _ => return None,
        };
        id = id << 4 | value as u64;
        digits += 1;
    }
    if digits == 16 { Some(id) } else { None }
}

/// `a == b`, in a const context.
const fn eq(a: &str, b: &str) -> bool {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a, b) {
            ([], []) => return true,
            ([x, rest_a @ ..], [y, rest_b @ ..]) if *x == *y => {
                a = rest_a;
                b = rest_b;
            }
            _ => return false,
        }
    }
}
