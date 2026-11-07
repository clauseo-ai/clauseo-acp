//! The CA bundle tier B points codex at (`CODEX_CA_CERTIFICATE`, `SSL_CERT_FILE`, S15 PR5).
//!
//! Codex verifies TLS with rustls, which reads the Windows certificate store; under the restricted
//! token it loads no roots from it. So the app exports the machine's roots once per start to
//! `<data_dir>\sandbox-ca\machine-<16 hex>.pem` ([`machine_name`], `Bundle::path`) and the
//! runner names that file.
//!
//! - The export (Windows, `store.rs`): the `Root` and `Disallowed` stores, opened read-only, at
//!   the local machine, its group policy and enterprise locations, and the current user and its
//!   group policy location; a store that doesn't exist is skipped. Each certificate's DER.
//! - The store properties (Windows, before the filter): a root whose usages are all turned off
//!   or leave out server authentication, whose disallowed usages list it, or that has a
//!   disallowed time is dropped ([`Filtered::dropped_limited`]); a missing property limits
//!   nothing.
//! - The filter ([`filter_roots`], every OS): a root also in `Disallowed` (the same DER, or the
//!   same public key) is dropped; only self-issued certificates (issuer Name bytes equal to the
//!   subject Name bytes) are kept, so no intermediate is ever trusted as a root; what doesn't parse
//!   as a certificate (`der.rs`, definite lengths only) is dropped; the rest de-duplicated, sorted
//!   by DER and written as PEM.
//! - The file (Windows): the folder made with the sandbox folder's protected DACL
//!   (`private_dir::create`, else repaired by `private_dir::protect`); the file written beside its
//!   final name with its own descriptor ([`file_sddl`]: owned by the desktop user, full access for
//!   it and `SYSTEM`, read for each pool user, no mandatory label, so a Low token may read it),
//!   then renamed to it. The name hashes the bytes and the pool, so a new export never renames
//!   over a file an agent holds open; a file already there with the same bytes is used as it
//!   is, and one that can't be read or differs is passed over for a numbered name. Older
//!   machine bundles are then removed, best-effort; one still open stays.
//! - The cache (Windows, `ensure`): once per process, and again when the pool's SIDs change or the
//!   file is gone.
//!
//! No proxy CA goes in the bundle: the egress proxy (S16, `proxy.rs`) tunnels TLS and never
//! terminates it. `SSL_CERT_FILE` also makes OpenSSL-based tools in the jail trust exactly the
//! machine's roots, which is intended (0049).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::sids::Sid;

mod der;
mod merge;
#[cfg(windows)]
mod store;

pub use merge::{Merged, machine_name, merge, merged_name};
#[cfg(windows)]
pub use store::{Bundle, disallowed, ensure, ensure_with, write_merged};

/// The bundle's folder in the data folder.
pub const CA_DIR: &str = "sandbox-ca";
/// The fixed bundle name earlier builds wrote in [`CA_DIR`]; each export now writes its own name
/// ([`machine_name`]) and removes this one when it can.
pub const CA_FILE: &str = "roots.pem";

/// `<data_dir>\sandbox-ca\roots.pem`, the fixed name of earlier builds. Not what a launch names:
/// that is `Bundle::path`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(CA_DIR).join(CA_FILE)
}

/// What [`filter_roots`] kept, as PEM, and how many distinct certificates each rule dropped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filtered {
    /// The kept certificates, sorted by DER, each a `CERTIFICATE` block in 64-column base64 with
    /// `\n` line ends. Empty when none is kept.
    pub pem: String,
    /// The certificates in `pem`.
    pub kept: usize,
    /// Dropped: in `Disallowed` by DER or by public key.
    pub dropped_disallowed: usize,
    /// Dropped: issued by another name, an intermediate.
    pub dropped_not_self_issued: usize,
    /// Dropped: not a certificate `der.rs` can read.
    pub dropped_unparsable: usize,
    /// Dropped before the filter (Windows export): a root whose store properties limit it for
    /// server authentication (its usages, disallowed usages or disallowed time). Always 0 from
    /// [`filter_roots`] itself.
    pub dropped_limited: usize,
}

/// Server authentication's OID, as `CertGetEnhancedKeyUsage` writes it.
pub const SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
/// [`SERVER_AUTH`]'s content octets in DER.
const SERVER_AUTH_DER: [u8; 8] = [0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];

/// Whether the DER usage list `encoded` (`SEQUENCE OF OBJECT IDENTIFIER`, as a certificate
/// store keeps disallowed usages) lists server authentication; `None` when it doesn't read.
pub fn lists_server_auth(encoded: &[u8]) -> Option<bool> {
    let oids = der::oid_list(encoded)?;
    Some(oids.contains(&&SERVER_AUTH_DER[..]))
}

/// The roots to trust from the `Root` stores' certificates `roots` and the `Disallowed` stores'
/// `disallowed`, all DER. Duplicates count once. The order of checks: unparsable, then
/// disallowed, then not self-issued; each certificate counts under the first that drops it.
pub fn filter_roots(roots: &[Vec<u8>], disallowed: &[Vec<u8>]) -> Filtered {
    let banned_der: BTreeSet<&[u8]> = disallowed.iter().map(Vec::as_slice).collect();
    let banned_spki: BTreeSet<&[u8]> = disallowed
        .iter()
        .filter_map(|cert| der::certificate(cert).map(|parts| parts.spki))
        .collect();
    let distinct: BTreeSet<&[u8]> = roots.iter().map(Vec::as_slice).collect();
    let mut out = Filtered::default();
    let mut kept = Vec::new();
    for cert in distinct {
        let Some(parts) = der::certificate(cert) else {
            out.dropped_unparsable += 1;
            continue;
        };
        if is_disallowed(cert, parts.spki, &banned_der, &banned_spki) {
            out.dropped_disallowed += 1;
        } else if parts.issuer != parts.subject {
            out.dropped_not_self_issued += 1;
        } else {
            kept.push(cert);
        }
    }
    out.kept = kept.len();
    out.pem = kept.into_iter().map(pem_block).collect();
    out
}

/// Whether the certificate `cert`, whose `SubjectPublicKeyInfo` TLV is `spki`, is disallowed: its
/// DER is in `banned_der`, or its public key in `banned_spki`.
fn is_disallowed(
    cert: &[u8],
    spki: &[u8],
    banned_der: &BTreeSet<&[u8]>,
    banned_spki: &BTreeSet<&[u8]>,
) -> bool {
    banned_der.contains(cert) || banned_spki.contains(spki)
}

/// One `CERTIFICATE` PEM block for `der`.
fn pem_block(der: &[u8]) -> String {
    let text = STANDARD.encode(der);
    let mut block = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in text.as_bytes().chunks(64) {
        block.push_str(&String::from_utf8_lossy(line));
        block.push('\n');
    }
    block.push_str("-----END CERTIFICATE-----\n");
    block
}

/// The bundle file's security descriptor: owned by the desktop user, a protected DACL with full
/// access for it and `SYSTEM` and read for each pool SID (sorted, each once). No label: a file
/// without one is Medium no-write-up only, so a Low pool token may read it and not write it.
pub fn file_sddl(desktop_user: &Sid, pool: &[Sid]) -> String {
    let user = desktop_user.as_str();
    let readers: BTreeSet<&str> = pool.iter().map(Sid::as_str).collect();
    let mut sddl = format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)");
    for reader in readers {
        sddl.push_str("(A;;FR;;;");
        sddl.push_str(reader);
        sddl.push(')');
    }
    sddl
}

#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod tests;

// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
