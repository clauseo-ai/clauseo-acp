//! A firm's CA files merged into the machine's bundle (S15 PR5): when a launch carries a firm CA
//! (`LaunchMessage::firm_ca`, which R17 fills from `network.ca_certs`), its certificates are
//! appended to the machine roots, each once, so codex and OpenSSL tools trust the firm's
//! TLS-inspecting proxy as well as the machine's roots. A firm certificate must be a CA's (basic
//! constraints `cA`) and in no `Disallowed` store (by DER or public key); the machine roots keep
//! the export's own filter. Pure, so its tests run on every OS.

use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::{der, is_disallowed, pem_block};

/// The PEM block markers.
const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";

/// A machine bundle with a firm's certificates appended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    /// The machine bundle's certificates, then the firm's not already in it, as PEM.
    pub pem: String,
    /// The firm certificates added.
    pub firm_kept: usize,
    /// Firm certificates (or whole files) that didn't parse as a certificate, left out.
    pub firm_unparsable: usize,
    /// Firm certificates also in `Disallowed` (by DER or public key), left out.
    pub firm_disallowed: usize,
    /// Firm certificates that aren't a CA's (no basic constraints `cA`), left out.
    pub firm_not_ca: usize,
}

/// The machine bundle `machine` (PEM, as `ensure` wrote it) with the certificates of each firm
/// file in `firm` (PEM with one or more `CERTIFICATE` blocks, or one DER certificate) appended:
/// in file order, each certificate once, none already in the machine bundle, none in
/// `disallowed` (DER certificates, matched by DER or public key) and only CA certificates.
pub fn merge(machine: &[u8], firm: &[Vec<u8>], disallowed: &[Vec<u8>]) -> Merged {
    let banned_der: BTreeSet<&[u8]> = disallowed.iter().map(Vec::as_slice).collect();
    let banned_spki: BTreeSet<&[u8]> = disallowed
        .iter()
        .filter_map(|cert| der::certificate(cert).map(|parts| parts.spki))
        .collect();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut out = Merged::default();
    for cert in certificates(machine).0 {
        if seen.insert(cert.clone()) {
            out.pem.push_str(&pem_block(&cert));
        }
    }
    for file in firm {
        let (certs, unparsable) = certificates(file);
        out.firm_unparsable += unparsable;
        for cert in certs {
            let spki = der::certificate(&cert)
                .map(|parts| parts.spki)
                .unwrap_or_default();
            if is_disallowed(&cert, spki, &banned_der, &banned_spki) {
                out.firm_disallowed += 1;
            } else if !der::is_ca(&cert) {
                out.firm_not_ca += 1;
            } else if seen.insert(cert.clone()) {
                out.pem.push_str(&pem_block(&cert));
                out.firm_kept += 1;
            }
        }
    }
    out
}

/// The merged bundle's file name for `pem` and the pool `pool` its descriptor names:
/// `roots-<16 hex>.pem`, from an FNV-1a hash of both, so one bundle serves every launch with
/// the same firm files and pool, and a grown pool gets a new file naming every pool user.
pub fn merged_name(pem: &str, pool: &[super::Sid]) -> String {
    format!("roots-{:016x}.pem", name_hash(pem.as_bytes(), pool))
}

/// The machine bundle's file name for `pem` and the pool `pool`: `machine-<16 hex>.pem`, hashed
/// as [`merged_name`] is. Each export with other roots or another pool gets a new name, so an
/// agent holding the last file open never blocks it (review L1).
pub fn machine_name(pem: &[u8], pool: &[super::Sid]) -> String {
    format!("machine-{:016x}.pem", name_hash(pem, pool))
}

/// FNV-1a over `pem` and the pool's SIDs, sorted, each once, joined by commas.
fn name_hash(pem: &[u8], pool: &[super::Sid]) -> u64 {
    let mut sids: Vec<&str> = pool.iter().map(super::Sid::as_str).collect();
    sids.sort_unstable();
    sids.dedup();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in pem.iter().copied().chain(sids.join(",").bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The certificates in `file` that parse, and how many blocks (or the whole file) didn't.
fn certificates(file: &[u8]) -> (Vec<Vec<u8>>, usize) {
    let text = String::from_utf8_lossy(file);
    let mut certs = Vec::new();
    let mut unparsable = 0;
    let mut rest = text.as_ref();
    let mut blocks = 0;
    while let Some(start) = rest.find(BEGIN) {
        let after = rest.get(start + BEGIN.len()..).unwrap_or_default();
        let Some(end) = after.find(END) else {
            unparsable += 1;
            break;
        };
        blocks += 1;
        let body: String = after
            .get(..end)
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        match STANDARD.decode(body) {
            Ok(der) if der::certificate(&der).is_some() => certs.push(der),
            _ => unparsable += 1,
        }
        rest = after.get(end + END.len()..).unwrap_or_default();
    }
    if blocks == 0 && unparsable == 0 && !file.is_empty() {
        // No PEM block: one DER certificate, or nothing usable.
        if der::certificate(file).is_some() {
            certs.push(file.to_vec());
        } else {
            unparsable += 1;
        }
    }
    (certs, unparsable)
}
