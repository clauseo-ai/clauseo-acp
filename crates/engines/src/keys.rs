//! The updater public key a list must be signed with, fixed at compile time the way
//! `crates/sandbox/build.rs` fixes the admin copy's: `CLAUSEO_UPDATER_PUBKEY` when the build sets
//! it (release.yml sets it per mode, so a test release trusts the test key), else
//! `plugins.updater.pubkey` from `crates/desktop/tauri.conf.json`. Either is Tauri's form of a
//! minisign public key: base64 of the `.pub` file (`untrusted comment: minisign public key: …`,
//! then the `RW…` key line). ASCII whitespace is taken out first, so a trailing line break in the
//! variable doesn't count; an empty value counts as unset.
//!
//! This crate has no build script (it's held to no I/O, and core-io bans build scripts there),
//! so the two sources are compiled in directly: the variable through `option_env!`, which cargo
//! tracks, and the config file through `include_str!`, read when the key is asked for. Unlike
//! the sandbox's build script this neither falls back to a placeholder (with no key, [`load`]
//! is [`Error::NoKey`](crate::Error::NoKey)) nor applies the release guard, which the sandbox's
//! build applies to the same variable in a release build.
//!
//! [`load`]: crate::load

/// `CLAUSEO_UPDATER_PUBKEY` as the build saw it.
pub const UPDATER_KEY_FROM_ENV: Option<&str> = option_env!("CLAUSEO_UPDATER_PUBKEY");

/// `crates/desktop/tauri.conf.json`, as the build saw it.
pub const TAURI_CONF: &str = include_str!("../../desktop/tauri.conf.json");

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
