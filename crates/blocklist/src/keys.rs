//! The public keys a list may be signed with, compiled in from minisign public key files
//! (`crates/blocklist/keys/*.pub`: an `untrusted comment:` line, then the `RW…` key line), so
//! stage F's release job verifies against the same bytes.
//!
//! Rotation (ADR 0053): a release adds the new key's file and its entry beside the old one, the
//! publisher switches to the new key once that release is out, and a later release drops the
//! old key. A list signed by any one key in the set verifies, so the releases overlap.

/// Stage F's key file for `clauseo-ai/blocklist` (minisign key id E8E787F0560990DA). The secret
/// key is only in that repo's GitHub secrets.
pub const STAGE_F_KEY_FILE: &str = include_str!("../keys/stage-f.pub");

/// Every key a list may be signed with: key files, or bare key lines. [`key_line`] takes the
/// key line of each; an entry with no key line is skipped.
pub const PUBLIC_KEYS: &[&str] = &[STAGE_F_KEY_FILE];

/// The key line (`RW…`) of a minisign public key file: its last non-empty line, trimmed. A bare
/// key line is its own key line. `None` when there is no such line.
pub fn key_line(file: &str) -> Option<&str> {
    file.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .filter(|line| !line.starts_with("untrusted comment:"))
}
