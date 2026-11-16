//! macOS: the trust roots a sandboxed Claude command's `curl` and Python read (decision record
//! 0030, "Claude's sandbox"). Inside Claude Code's Seatbelt sandbox they cannot reach the macOS
//! trust store and fall back to `/etc/ssl/cert.pem`, macOS's stale bundle, which lacks newer
//! roots such as `GlobalSign Root R46`: `sci.gov.in` and many Indian government sites then fail
//! with "self signed certificate" (stage C, 2026-11-11; the chain was the same inside and out,
//! so no interception). `curl --cacert` with the System Roots keychain exported fetched the
//! PDF inside the sandbox.
//!
//! So, for a Claude session on macOS whose sandbox is on, the launch prep writes [`ROOTS_FILE`]
//! under the data folder, rewritten each launch: the certificates of the System Roots keychain,
//! then the ones an admin added to the System keychain (a firm's TLS-inspection root), each
//! exported by `/usr/bin/security find-certificate -a -p` under a time limit. The session's
//! environment and its shells' file then point [`CA_VARS`] at it (`agent::shell_path`), and the
//! sandbox's `allowRead` opens that one file (`sandbox.rs`). When the System Roots export fails
//! or holds no certificate, nothing is set (one warning): `curl` stays as it was. Linux and
//! Windows are unchanged.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::stetdoc::{RunOptions, run_for};

use crate::clock::{Clock, ProcessTimer};
use crate::log_file::Cause;

/// The file, under the data folder.
pub(crate) const ROOTS_FILE: [&str; 2] = ["tls", "roots.pem"];

/// The variables `curl`, OpenSSL and Python's `requests` read a CA bundle from.
pub(crate) const CA_VARS: [&str; 3] = ["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"];

/// The System Roots keychain, which must export.
pub(crate) const SYSTEM_ROOTS: &str = "/System/Library/Keychains/SystemRootCertificates.keychain";

/// The System keychain, where an admin's added roots are; it may hold none.
pub(crate) const SYSTEM_KEYCHAIN: &str = "/Library/Keychains/System.keychain";

/// The `security` tool.
const SECURITY: &str = "/usr/bin/security";

/// How long one export may take.
const TIME_LIMIT: Duration = Duration::from_secs(10);

/// The most an export may print: the System Roots are about 250 KB.
const STDOUT_MAX: u64 = 8 * 1024 * 1024;

/// A PEM certificate's first line.
const BEGIN: &str = "-----BEGIN CERTIFICATE-----";

/// Writes [`ROOTS_FILE`] under `data` from `export` of [`SYSTEM_ROOTS`] then
/// [`SYSTEM_KEYCHAIN`] (a keychain's PEM text, or none when the export failed) and gives its
/// path; none, with one warning, when the System Roots give no certificate or the write fails.
/// File system calls: off the actor.
pub(crate) fn write_roots(data: &Path, export: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let Some(roots) = export(SYSTEM_ROOTS).filter(|pem| pem.contains(BEGIN)) else {
        tracing::warn!("the System Roots did not export; Claude's sandboxed curl keeps its bundle");
        return None;
    };
    let mut pem = roots;
    if let Some(added) = export(SYSTEM_KEYCHAIN).filter(|pem| pem.contains(BEGIN)) {
        if !pem.ends_with('\n') {
            pem.push('\n');
        }
        pem.push_str(&added);
    }
    let folder = data.join(ROOTS_FILE[0]);
    let file = folder.join(ROOTS_FILE[1]);
    let written = crate::write::create_private_dirs_under(data, &folder)
        .and_then(|()| crate::write::replace_private_file(&file, pem.as_bytes()));
    match written {
        Ok(()) => Some(file),
        Err(error) => {
            tracing::warn!(
                error = %Cause(&error),
                "the trust roots file was not written; Claude's sandboxed curl keeps its bundle"
            );
            None
        }
    }
}

/// An export that runs `security find-certificate -a -p <keychain>` in a cleared environment
/// within [`TIME_LIMIT`] on `clock` (killed past it, with its tree) and gives what it printed,
/// or none when it failed, timed out, printed more than [`STDOUT_MAX`] or no UTF-8. Blocks the
/// calling thread (the launch prep's) while it runs.
pub(crate) fn security(clock: Arc<dyn Clock>) -> impl Fn(&str) -> Option<String> {
    move |keychain| {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().clear_env().end_with_parent());
        let timer = ProcessTimer(Arc::clone(&clock));
        let options = RunOptions {
            limit: TIME_LIMIT,
            stdout_max: STDOUT_MAX,
            env: std::collections::BTreeMap::new(),
        };
        let args = ["find-certificate", "-a", "-p", keychain].map(OsString::from);
        let program = Path::new(SECURITY);
        let ran = futures::executor::block_on(run_for(&launcher, &timer, program, &args, &options));
        let ran = ran.ok().filter(|ran| ran.code == Some(0))?;
        String::from_utf8(ran.stdout).ok()
    }
}

#[cfg(test)]
#[path = "tls_roots_tests.rs"]
mod tests;
