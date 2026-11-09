//! A jailed launch's CA bundle (R17 PR-D): one PEM file of the OS's roots and the agent's
//! `network.ca_certs` file, the set desktop's launch hook gives an unjailed agent today
//! (`NODE_USE_SYSTEM_CA=1` for the OS's store, the CA file under the CLI's own variable). In the
//! jail the OS's store is out of reach (no Keychain, no `trustd`, ruling of 2026-11-08) and the
//! CA file may be anywhere, so the host writes both into `run/<session>/ca-bundle.pem`, grants
//! that one file read-only, and points [`CA_VARS`] at it. Written off the actor, on the seam's
//! thread, and gone with the run folder.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::config::sandbox::CaRoots;
use crate::log_file::Cause;

/// The bundle's name in the run folder.
pub(crate) const CA_FILE: &str = "ca-bundle.pem";

/// The variables set to the bundle: what tier B's runner sets (`CODEX_CA_CERTIFICATE` and
/// `SSL_CERT_FILE`, `clauseo_sandbox::windows` `CA_VARS`), and Node's extra CA file
/// (`NODE_EXTRA_CA_CERTS`), which desktop's hook points at the agent's `network.ca_certs` file,
/// a path the jail does not grant.
pub(crate) const CA_VARS: [&str; 3] = [
    "CODEX_CA_CERTIFICATE",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
];

/// The OS's roots, as `rustls-native-certs` loads them; a root it could not read is left out with
/// one log line naming the count only.
pub(crate) fn native_roots() -> Vec<Vec<u8>> {
    let found = rustls_native_certs::load_native_certs();
    if !found.errors.is_empty() {
        let errors = found.errors.len();
        tracing::warn!(errors, "some of the OS's trusted roots could not be read");
    }
    found
        .certs
        .into_iter()
        .map(|cert| cert.as_ref().to_vec())
        .collect()
}

/// The PEM certificate blocks of `text`, each from its `BEGIN CERTIFICATE` line to its
/// `END CERTIFICATE` line, and nothing else: a private key, or any other block or text, in the
/// same file never reaches the jail. A block with no end is left out.
pub(crate) fn certificates(text: &str) -> Vec<String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some((_, opened)) = rest.split_once(BEGIN) {
        let Some((body, after)) = opened.split_once(END) else {
            break;
        };
        // A BEGIN with no END of its own before this one: only the last BEGIN's block counts.
        let body = body.rsplit_once(BEGIN).map_or(body, |(_, last)| last);
        blocks.push(format!("{BEGIN}{body}{END}"));
        rest = after;
    }
    blocks
}

/// The bundle's text: each of `roots` (DER) as a PEM certificate, then the `extra` file's
/// certificate blocks ([`certificates`]), each as written.
pub(crate) fn bundle(roots: &[Vec<u8>], extra: Option<&str>) -> String {
    let mut pem = String::new();
    for der in roots {
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        let text = STANDARD.encode(der);
        for line in text.as_bytes().chunks(64) {
            pem.push_str(&String::from_utf8_lossy(line));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
    }
    for block in extra.map(certificates).unwrap_or_default() {
        pem.push_str(&block);
        pem.push('\n');
    }
    pem
}

/// Writes the bundle for a launch into `run_root` (`run/<session>`): the roots `roots` gives
/// (the OS's when `None`), and the certificate blocks of the agent's CA file `extra`, only
/// them (one with none is left out, with a log line). The file's path, or `None` when it could
/// not be written (the agent then trusts what its own runtime ships). File system calls: off
/// the actor.
pub(crate) fn write(
    run_root: &Path,
    roots: Option<&CaRoots>,
    extra: Option<&Path>,
) -> Option<PathBuf> {
    let roots = roots.map_or_else(native_roots, |roots| roots());
    let extra = extra.and_then(|file| match std::fs::read_to_string(file) {
        Ok(text) if !certificates(&text).is_empty() => Some(text),
        Ok(_) => {
            tracing::warn!("the agent's CA file holds no PEM certificate; the jail goes without it");
            None
        }
        Err(error) => {
            let error = Cause(&error);
            tracing::warn!(%error, "the agent's CA file could not be read; the jail goes without it");
            None
        }
    });
    let file = run_root.join(CA_FILE);
    match crate::write::replace_private_file(&file, bundle(&roots, extra.as_deref()).as_bytes()) {
        Ok(()) => Some(file),
        Err(error) => {
            let error = Cause(&error);
            tracing::warn!(%error, "the jail's CA bundle could not be written");
            None
        }
    }
}

#[cfg(test)]
mod tests;
