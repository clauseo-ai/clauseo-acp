//! Whether a path the agent chose names a network location, decided from its text alone,
//! before anything opens, stats or resolves it: touching a share as the lawyer makes their
//! machine sign in to a server the agent picked (NTLM sent to it on Windows; an autofs or smbfs
//! mount on macOS). [`is_network_path`] is pure: the remote mount list and the drive-type
//! answer come in a [`NetEnv`], read once per check by [`NetEnv::current`]. R18 3b's picker
//! reuses it.
//!
//! Network, on every system:
//! - Windows forms: a UNC path (`\\server\share`), `\\?\UNC\…`, `\??\…`, `\Device\…` (such as
//!   `\Device\Mup\…`), `\GLOBAL??\…`, and every other device-namespace form (`\\.\…`,
//!   `\\?\GLOBALROOT\…`, `\\?\Volume{…}\…`). Of the `\\?\` forms only `\\?\X:\…` is a local path.
//!   With [`NetEnv::windows`], `/` separates too (`//server/share` is UNC) and a drive whose
//!   root `GetDriveTypeW` calls `DRIVE_REMOTE` is network.
//! - `/net/…` and `/Network/…` (macOS's and Linux's automounted hosts).
//! - A URL (`smb://`, `afp://`, `nfs://`, `file://` and any other scheme).
//! - A path at or under a remote mount point ([`NetEnv::remote_mounts`]): on macOS each mount
//!   without `MNT_LOCAL`, or of type `smbfs`, `nfs`, `afpfs`, `webdav` or `autofs`; on Linux
//!   each `/proc/self/mounts` entry of a network type (`LINUX_REMOTE_TYPES`).

use std::fmt;
use std::path::{Path, PathBuf};

pub(crate) use share::{may_touch, under_share_matter};

#[cfg(target_os = "macos")]
mod mac;
pub(crate) mod resolve;
mod share;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(windows)]
mod windows;

/// The Linux file system types that live on another machine.
#[cfg(any(target_os = "linux", test))]
pub(crate) const LINUX_REMOTE_TYPES: [&str; 12] = [
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "fuse.sshfs",
    "sshfs",
    "afs",
    "ceph",
    "glusterfs",
    "fuse.glusterfs",
    "davfs",
];

/// What [`is_network_path`] compares a path against.
pub(crate) struct NetEnv {
    /// Windows' rules too: `/` separates names, `//server/share` is UNC, drives are asked.
    pub(crate) windows: bool,
    /// Names compare without case (macOS and Windows volumes).
    pub(crate) fold_case: bool,
    /// The mount points of remote file systems.
    pub(crate) remote_mounts: Vec<PathBuf>,
    /// Whether drive `X:` is a network drive. Given the letter alone, so it can only ask about
    /// the drive's root (`GetDriveTypeW` on `X:\`), never a path on it.
    pub(crate) remote_drive: Box<dyn Fn(char) -> bool + Send>,
}

impl fmt::Debug for NetEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetEnv")
            .field("windows", &self.windows)
            .field("fold_case", &self.fold_case)
            .field("remote_mounts", &self.remote_mounts.len())
            .finish_non_exhaustive()
    }
}

impl NetEnv {
    /// This system's: its mount list read now, its drives asked as a path names them. Reads
    /// the mount list (macOS `getfsstat`, Linux `/proc/self/mounts`), which never opens a path.
    pub(crate) fn current() -> Self {
        Self {
            windows: cfg!(windows),
            fold_case: FOLD_CASE,
            remote_mounts: remote_mounts(),
            remote_drive: Box::new(remote_drive),
        }
    }

    /// Whether `path` names a network location ([`is_network_path`]).
    pub(crate) fn is_network(&self, path: &Path) -> bool {
        is_network_path(path, self)
    }

    /// `path` with a `\\?\X:\` prefix put in its plain `X:\` form (with Windows' rules), so the
    /// names after it join and `..` as in any other path; as it is otherwise.
    pub(crate) fn plain(&self, path: &Path) -> PathBuf {
        let text = path.to_string_lossy();
        let rest = text
            .strip_prefix(r"\\?\")
            .or_else(|| text.strip_prefix("//?/"));
        match rest {
            Some(rest) if self.windows && drive_of(rest).is_some() => PathBuf::from(rest),
            _ => path.to_path_buf(),
        }
    }
}

/// Whether `path` names a network location under `net`'s rules. Pure text: it never touches
/// the disk, and asks `net.remote_drive` only for a drive letter.
pub(crate) fn is_network_path(path: &Path, net: &NetEnv) -> bool {
    let text = path.to_string_lossy();
    if is_url(&text) {
        return true;
    }
    // Backslash forms on every system: on Unix such a text is a file name nobody makes, and
    // the same text read as a Windows path names a server.
    let windows_text = if net.windows {
        text.replace('/', "\\")
    } else {
        text.to_string()
    };
    if let Some(rest) = windows_text.strip_prefix(r"\\?\") {
        // Of the verbatim forms only `\\?\X:\…` names a local path, if its drive is local.
        return match drive_of(rest) {
            Some(letter) => {
                let after = rest.get(2..).unwrap_or_default();
                !(after.is_empty() || after.starts_with('\\')) || (net.remote_drive)(letter)
            }
            None => true,
        };
    }
    if windows_text.starts_with(r"\\") || windows_text.starts_with(r"\??\") {
        return true;
    }
    if let Some(rest) = windows_text.strip_prefix('\\') {
        let first = rest.split('\\').next().unwrap_or_default();
        if first.eq_ignore_ascii_case("Device") || first.eq_ignore_ascii_case("GLOBAL??") {
            return true;
        }
    }
    if net.windows
        && let Some(letter) = drive_of(&windows_text)
        && (net.remote_drive)(letter)
    {
        return true;
    }
    if let Some(rest) = text.strip_prefix('/') {
        let first = rest.split('/').next().unwrap_or_default();
        let named =
            |want: &str| first == want || (net.fold_case && first.eq_ignore_ascii_case(want));
        if named("net") || named("Network") {
            return true;
        }
    }
    net.remote_mounts
        .iter()
        .any(|mount| under(path, mount, net.fold_case))
}

/// Whether `path` is `prefix` or inside it, by whole components; without case when
/// `fold_case`.
pub(crate) fn under(path: &Path, prefix: &Path, fold_case: bool) -> bool {
    if !fold_case {
        return path.starts_with(prefix);
    }
    let mut names = path.components();
    prefix.components().all(|want| {
        names.next().is_some_and(|got| {
            let (got, want) = (got.as_os_str(), want.as_os_str());
            got == want
                || got.to_string_lossy().to_lowercase() == want.to_string_lossy().to_lowercase()
        })
    })
}

/// `name` folded for a match that refuses (a deny by name or prefix): at least as broadly as
/// APFS and NTFS fold names, so a spelling the file system takes as the same name always
/// matches. Each character lowercased (`char::to_lowercase`: ASCII, the Kelvin sign `K`
/// U+212A to `k`, and every other cased letter), and the long s `ſ` (U+017F) made `s`.
/// Over-matching is the safe side for a deny; never use it for a match that allows
/// ([`under_share_matter`] compares strictly).
pub(crate) fn fold_name(name: &str) -> String {
    name.chars()
        .flat_map(|c| match c {
            '\u{17F}' => 's'.to_lowercase(),
            c => c.to_lowercase(),
        })
        .collect()
}

/// Folds the case of names on the volumes that ignore it: macOS and Windows.
pub(crate) const FOLD_CASE: bool = cfg!(any(target_os = "macos", windows));

/// The drive letter `text` starts with (`C:`), upper case.
fn drive_of(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':')).then(|| letter.to_ascii_uppercase())
}

/// Whether `text` starts with a URL scheme of two or more characters and `://` (one letter is
/// a drive: `C://x`).
pub(crate) fn is_url(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    scheme.len() >= 2
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The mount points of the remote file systems in a Linux mount table (`/proc/self/mounts`):
/// `device mountpoint type options 0 0`, the mount point's space, tab, newline and `\` written
/// as octal escapes.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_remote_mounts(table: &str) -> Vec<PathBuf> {
    table
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let (_, mount, kind) = (fields.next()?, fields.next()?, fields.next()?);
            LINUX_REMOTE_TYPES
                .contains(&kind)
                .then(|| PathBuf::from(unescape_octal(mount)))
        })
        .collect()
}

/// `\040`-style escapes made the characters they stand for.
#[cfg(any(target_os = "linux", test))]
fn unescape_octal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('\\') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let digits = rest.get(at.saturating_add(1)..at.saturating_add(4));
        let code = digits
            .filter(|d| d.bytes().all(|b| (b'0'..=b'7').contains(&b)))
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        if let Some(code) = code {
            out.push(char::from(code));
            rest = rest.get(at.saturating_add(4)..).unwrap_or_default();
        } else {
            out.push('\\');
            rest = rest.get(at.saturating_add(1)..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    out
}

#[cfg(target_os = "linux")]
fn remote_mounts() -> Vec<PathBuf> {
    match std::fs::read_to_string("/proc/self/mounts") {
        Ok(table) => linux_remote_mounts(&table),
        Err(error) => {
            tracing::warn!(%error, "the mount table could not be read; no mount counts as remote");
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
fn remote_mounts() -> Vec<PathBuf> {
    mac::remote_mounts()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn remote_mounts() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn remote_drive(letter: char) -> bool {
    windows::remote_drive(letter)
}

#[cfg(not(windows))]
fn remote_drive(_letter: char) -> bool {
    false
}
