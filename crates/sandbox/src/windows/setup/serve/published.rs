//! The set-up pipe's name, one per server run, and where the server publishes it (0049, "The
//! set-up task"). A fixed name could be made by any local account between two runs, which
//! would block every set-up until a restart; a name drawn at random for each run (128 bits) can't
//! be made first, as no one knows it before the server makes the pipe.
//!
//! The server makes the pipe `\\.\pipe\ClauseoSetUp-<32 hex digits>` ([`pipe_name`]), then
//! writes the hex digits to [`PUBLISHED_VALUE`] in [`PUBLISHED_SUBKEY`], below the record's key.
//! The record's key lets Administrators write; this subkey has its own protected descriptor,
//! [`PUBLISHED_SDDL`]: owned by LOCAL SYSTEM, which alone may write it, Users reading, and
//! Administrators allowed only to delete it (an uninstall's clean-up). It clears the value as
//! it exits; a run that was ended leaves it, naming a pipe that's gone, which the app reads as
//! stale (`client.rs`). No Windows call; tested on every OS.

/// The pipe's name before its run's hex digits.
pub const PIPE_PREFIX: &str = r"\\.\pipe\ClauseoSetUp-";
/// How many random bytes name a run: 128 bits.
pub const TOKEN_BYTES: usize = 16;
/// The subkey of the record's key (`HKLM\SOFTWARE\Clauseo\Sandbox`) that holds the name.
pub const PUBLISHED_SUBKEY: &str = "SetUpPipe";
/// The value in it: the running server's hex digits, a `REG_SZ`.
pub const PUBLISHED_VALUE: &str = "Pipe";
/// The subkey's descriptor: owned by LOCAL SYSTEM, the only writer; Users read; Administrators
/// may delete it, nothing more. Protected, so the record key's own entries aren't inherited.
pub const PUBLISHED_SDDL: &str = "O:SYD:P(A;;KA;;;SY)(A;;SD;;;BA)(A;;KR;;;BU)";

/// The run's token: `bytes` as lowercase hex, 32 digits.
pub fn token(bytes: &[u8; TOKEN_BYTES]) -> String {
    crate::windows::admin_copy::hex(bytes)
}

/// The pipe a published `token` names; `None` unless it is exactly 32 lowercase hex digits, so
/// a value that isn't one never names another object.
pub fn pipe_name(token: &str) -> Option<String> {
    let well_formed = token.len() == 2 * TOKEN_BYTES
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    well_formed.then(|| format!("{PIPE_PREFIX}{token}"))
}
