//! The outer runner's CA step (Windows): a firm's CA files merged into the machine bundle
//! (`ca_bundle::merge`), written beside it with the bundle's own descriptor, and the bundle
//! variables pointed at the merged file. A firm file that can't be read, or a merge that can't
//! be written, leaves the machine bundle in place with one line on stderr: a missing firm CA
//! trusts less, never more.

use std::io::{self, Read as _};
use std::path::Path;

use super::LaunchMessage;
use super::env::{CA_VARS, firm_ca_files};
use super::paths::WinPath;
use crate::windows::ca_bundle::{merge, write_merged};
use crate::windows::sids::Sid;

/// The largest machine bundle read.
const BUNDLE_CAP: u64 = 8 * 1024 * 1024;
/// The largest firm CA file read.
const FIRM_CAP: u64 = 1024 * 1024;

/// `env` with the bundle variables pointed at the merged bundle when the launch has firm CA
/// files and a bundle; else `env` as it is. `say` gets each line for stderr.
pub(super) fn with_firm_ca(
    message: &LaunchMessage,
    desktop: &Sid,
    mut env: Vec<(String, String)>,
    say: impl Fn(&str),
) -> Vec<(String, String)> {
    let firm = firm_ca_files(message);
    let Some(bundle) = message.ca_bundle.as_deref() else {
        return env;
    };
    if firm.is_empty() {
        return env;
    }
    match merged(bundle, &firm, desktop, &say) {
        Ok(Some(path)) => {
            for (name, value) in &mut env {
                if CA_VARS.iter().any(|var| var.eq_ignore_ascii_case(name)) {
                    value.clone_from(&path);
                }
            }
        }
        Ok(None) => {}
        Err(error) => say(&format!(
            "clauseo-sandbox-run: the firm CA wasn't merged ({}); the machine bundle stays",
            error.kind()
        )),
    }
    env
}

/// Writes the merged bundle and gives its path; `None` when no firm certificate was added.
fn merged(
    bundle: &str,
    firm: &[String],
    desktop: &Sid,
    say: &impl Fn(&str),
) -> io::Result<Option<String>> {
    let machine = read_capped(Path::new(bundle), BUNDLE_CAP)?;
    let mut files = Vec::new();
    for (index, file) in firm.iter().enumerate() {
        match read_capped(Path::new(file), FIRM_CAP) {
            Ok(bytes) => files.push(bytes),
            // The file's place may be the lawyer's profile; name it by its index.
            Err(error) => say(&format!(
                "clauseo-sandbox-run: firm CA file {index} left out ({})",
                error.kind()
            )),
        }
    }
    let merged = merge(&machine, &files);
    if merged.firm_unparsable > 0 {
        say(&format!(
            "clauseo-sandbox-run: {} firm CA entries didn't parse and were left out",
            merged.firm_unparsable
        ));
    }
    if merged.firm_kept == 0 {
        return Ok(None);
    }
    let pool = crate::windows::sandbox_pool_sids().map_err(io::Error::other)?;
    // `<data_dir>\sandbox-ca\roots.pem`: the merged file goes beside it.
    let data_dir = Path::new(bundle)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no bundle folder"))?;
    let path = write_merged(data_dir, desktop, &pool, &merged.pem)?;
    let text = path
        .to_str()
        .and_then(WinPath::parse)
        .map(|path| path.display())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a path that isn't UTF-8"))?;
    Ok(Some(text))
}

/// The bytes of `path`, refused past `cap`.
fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > cap {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
    }
    Ok(bytes)
}
