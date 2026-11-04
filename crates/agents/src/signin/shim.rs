//! Claude's `BROWSER` shim (macOS and Linux), and the one rule for which address is opened.
//!
//! The brief: "`BROWSER` names a small `sh` script at `<data_dir>/agents/signin/browser` that
//! appends its argument to a file named by `CLAUSEO_SIGNIN_URL_FILE`, created with mode 0600 for
//! each run and deleted when the run ends. A captured address whose host is in the snapshot's
//! sign-in hosts becomes `OpenUrl`". So the shim never opens anything itself: the login's job
//! reads the file and sends each new address once, after [`sign_in_address`] says yes.
//!
//! Addresses read from the login's output lines are never opened: Claude's is the paste route,
//! which goes with the paste field. Windows has no shim until G7 row 1 says Claude honours
//! `BROWSER` there; Claude opens its own tab and Clauseo opens nothing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use clauseo_protocol::AgentId;

/// The shim's text. It refuses to run without its file, and writes one line per address.
#[cfg(unix)]
const SCRIPT: &str = "#!/bin/sh\n\
# Clauseo's sign-in: records the address Claude would open, so Clauseo opens it.\n\
[ -n \"$CLAUSEO_SIGNIN_URL_FILE\" ] || exit 1\n\
printf '%s\\n' \"$1\" >> \"$CLAUSEO_SIGNIN_URL_FILE\"\n";

/// The most of a URL file read: an address list, never more than a few lines.
const URL_FILE_MAX: u64 = 64 * 1024;

/// `<data_dir>/agents/signin`: the shim and the runs' URL files.
#[cfg(unix)]
pub(crate) fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("signin")
}

/// `<data_dir>/agents/signin/browser`.
#[cfg(unix)]
pub(crate) fn script_path(data_dir: &Path) -> PathBuf {
    folder(data_dir).join("browser")
}

/// Writes the shim (mode 0700) at [`script_path`], replacing whatever is there, and gives its
/// path. Called by the worker thread alone, so two jobs never write it at once.
#[cfg(unix)]
pub(crate) fn install(data_dir: &Path) -> std::io::Result<PathBuf> {
    let path = script_path(data_dir);
    crate::write::create_private_dirs_under(data_dir, &folder(data_dir))?;
    crate::write::write_script(&path, SCRIPT.as_bytes())?;
    Ok(path)
}

/// One login run's URL file: made new with mode 0600 before the run, removed when this is
/// dropped, however the run ended.
#[derive(Debug)]
pub(crate) struct UrlFile {
    path: PathBuf,
    /// Addresses already sent, so each goes once.
    sent: BTreeSet<String>,
}

impl UrlFile {
    /// Makes `<data_dir>/agents/signin/url-<agent>-<job>`, removing a stale one first (a run
    /// of an earlier app that ended without its clean-up).
    #[cfg(unix)]
    pub(crate) fn create(data_dir: &Path, agent: &AgentId, job: u64) -> std::io::Result<UrlFile> {
        let path = folder(data_dir).join(format!("url-{agent}-{job}"));
        crate::write::create_private_dirs_under(data_dir, &folder(data_dir))?;
        crate::write::remove_file(&path)?;
        crate::write::create_private(&path)?;
        Ok(UrlFile {
            path,
            sent: BTreeSet::new(),
        })
    }

    /// Its path: `CLAUSEO_SIGNIN_URL_FILE`.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The addresses in the file not sent before whose host is one of `hosts`, in order. Each
    /// is then counted as sent. A file that cannot be read gives none this time.
    pub(crate) fn take_new(&mut self, hosts: &[String]) -> Vec<String> {
        let Some(text) = self.read() else {
            return Vec::new();
        };
        let mut new = Vec::new();
        for line in text.lines().map(str::trim) {
            if sign_in_address(line, hosts) && self.sent.insert(line.to_owned()) {
                new.push(line.to_owned());
            }
        }
        new
    }

    fn read(&self) -> Option<String> {
        use std::io::Read as _;
        let file = crate::write::open_read(&self.path).ok()?;
        let mut bytes = Vec::new();
        file.take(URL_FILE_MAX).read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

impl Drop for UrlFile {
    fn drop(&mut self) {
        // Nothing more can be done if it will not go; it holds an address, never a secret.
        drop(crate::write::remove_file(&self.path));
    }
}

/// Whether `url` may be opened: `https://`, a host that is exactly one of `hosts` (ASCII case
/// aside), an optional port, and nothing that could make a browser read another host: no user
/// name (`@` before the path), no backslash, no space or control character, no IP literal.
pub(crate) fn sign_in_address(url: &str, hosts: &[String]) -> bool {
    let Some(rest) = url
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
        .and_then(|_| url.get(8..))
    else {
        return false;
    };
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') || authority.starts_with('[') {
        return false;
    }
    let host = match authority.split_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => host,
        Some(_) => return false,
        None => authority,
    };
    !host.is_empty() && hosts.iter().any(|known| known.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests;
