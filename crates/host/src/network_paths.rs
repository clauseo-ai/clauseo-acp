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

use unicode_normalization::UnicodeNormalization as _;

pub(crate) use share::{may_touch, under_share_matter};

#[cfg(target_os = "macos")]
mod mac;
pub(crate) mod resolve;
mod share;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(windows)]
mod windows;

/// The Linux file system types that live on another machine, or mount one on first touch
/// (`autofs`), FUSE network file systems included.
#[cfg(any(target_os = "linux", test))]
pub(crate) const LINUX_REMOTE_TYPES: [&str; 21] = [
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
    "autofs",
    "fuse.rclone",
    "fuse.gvfsd-fuse",
    "fuse.s3fs",
    "fuse.ceph",
    "fuse.davfs2",
    "lustre",
    "ncpfs",
    "coda",
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
        let first = name_key(rest.split('/').next().unwrap_or_default());
        if first == "net" || first == "network" {
            return true;
        }
    }
    net.remote_mounts.iter().any(|mount| under(path, mount))
}

/// A name as a network check compares it: normalised to NFD (as APFS stores names), then
/// folded by [`fold_name`] (ASCII, the long s, the Kelvin sign, every cased letter), then NFD
/// again. On every system: a network check refuses, and over-matching is its safe side.
fn name_key(name: &str) -> String {
    let folded = fold_name(&name.nfd().collect::<String>());
    folded.nfd().collect()
}

/// Whether `path` is `prefix` (a remote mount point) or inside it, by whole components each
/// compared by [`name_key`], so a spelling the file system takes as the same name matches.
fn under(path: &Path, prefix: &Path) -> bool {
    let mut names = path.components();
    prefix.components().all(|want| {
        names.next().is_some_and(|got| {
            let (got, want) = (got.as_os_str(), want.as_os_str());
            got == want || name_key(&got.to_string_lossy()) == name_key(&want.to_string_lossy())
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

/// The mount points of the remote file systems in a Linux mount table (`/proc/self/mounts`),
/// read as bytes: `device mountpoint type options 0 0`, the mount point's space, tab, newline
/// and `\` written as octal escapes, its other bytes as they are (not always UTF-8). `None`
/// when a line does not parse: the table is then unknown, and the caller counts every path as
/// remote.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_remote_mounts(table: &[u8]) -> Option<Vec<PathBuf>> {
    let mut mounts = Vec::new();
    for line in table.split(|&b| b == b'\n') {
        let mut fields = line
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty());
        let Some(device) = fields.next() else {
            continue;
        };
        let (_, Some(mount), Some(kind)) = (device, fields.next(), fields.next()) else {
            return None;
        };
        if LINUX_REMOTE_TYPES.iter().any(|t| t.as_bytes() == kind) {
            mounts.push(path_of_bytes(unescape_octal(mount)));
        }
    }
    Some(mounts)
}

/// A path from its bytes: as they are on Unix, read as UTF-8 (lossily) elsewhere.
#[cfg(any(target_os = "linux", test))]
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(
            String::from_utf8(bytes)
                .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()),
        )
    }
}

/// `\040`-style escapes made the bytes they stand for.
#[cfg(any(target_os = "linux", test))]
fn unescape_octal(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        let digits = text.get(at.saturating_add(1)..at.saturating_add(4));
        let code = digits
            .filter(|_| byte == b'\\')
            .filter(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)))
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        if let Some(code) = code {
            out.push(code);
            at = at.saturating_add(4);
        } else {
            out.push(byte);
            at = at.saturating_add(1);
        }
    }
    out
}

/// What every path counts as under when the mount list can't be read: the root, so every path
/// is remote and refused (fail-closed).
#[cfg(any(target_os = "linux", target_os = "macos"))]
const UNKNOWN_MOUNTS: &str = "/";

#[cfg(target_os = "linux")]
fn remote_mounts() -> Vec<PathBuf> {
    let table = std::fs::read("/proc/self/mounts");
    table
        .as_deref()
        .ok()
        .and_then(linux_remote_mounts)
        .unwrap_or_else(|| {
            tracing::warn!("the mount table could not be read; every path counts as remote");
            vec![PathBuf::from(UNKNOWN_MOUNTS)]
        })
}

#[cfg(target_os = "macos")]
fn remote_mounts() -> Vec<PathBuf> {
    mac::remote_mounts().unwrap_or_else(|| vec![PathBuf::from(UNKNOWN_MOUNTS)])
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
