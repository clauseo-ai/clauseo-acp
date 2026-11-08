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
//!   device `QueryDosDeviceW` names as anything but a local volume (`\Device\HarddiskVolume…`,
//!   `\Device\CdRom…`) is network, as is one it can't name.
//! - `/net/…` and `/Network/…` (macOS's and Linux's automounted hosts).
//! - With [`NetEnv::macos`]: `/.nofollow/…`, `/.resolve/…` and `/.vol/…` (they reach a mount by
//!   another text), `/home/…` (`auto_home`), and `/System/Volumes/Data/…` put in its firmlinked
//!   form first (`/usr/share/firmlinks`; refused when that can't be read).
//! - A URL (`smb://`, `afp://`, `nfs://`, `file://` and any other scheme).
//! - A path at or under a remote mount point ([`NetEnv::remote_mounts`]): on macOS each mount
//!   without `MNT_LOCAL`, or of type `smbfs`, `nfs`, `afpfs`, `webdav` or `autofs`; on Linux
//!   each `/proc/self/mounts` entry whose type is not a known local one (`linux.rs`).
//!
//! Names a deny matches against an ASCII name (`/net`, `.nofollow`) fold as a case-insensitive
//! volume does ([`name_folds_to`]); mount names, which are not ASCII, compare NFD-normalised and
//! folded ([`fold_name`]).

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization as _;

pub(crate) use ascii_fold::name_folds_to;
pub(crate) use share::{may_touch, under_share_matter};
pub(crate) use windows_names::refused_windows_name;

mod ascii_fold;
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
mod macos_rules;
pub(crate) mod resolve;
mod share;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(windows)]
mod windows;
mod windows_names;

/// What [`is_network_path`] compares a path against.
pub(crate) struct NetEnv {
    /// Windows' rules too: `/` separates names, `//server/share` is UNC, drives are asked.
    pub(crate) windows: bool,
    /// Names compare without case (macOS and Windows volumes).
    pub(crate) fold_case: bool,
    /// The mount points of remote file systems.
    pub(crate) remote_mounts: Vec<PathBuf>,
    /// Whether drive `X:` is a network drive. Given the letter alone, so it can only ask what
    /// device the letter names (`QueryDosDeviceW`), never open a path on it.
    pub(crate) remote_drive: Box<dyn Fn(char) -> bool + Send>,
    /// macOS's rules too: `/.nofollow`, `/.resolve`, `/.vol`, `/home` and the firmlinks.
    pub(crate) macos: bool,
    /// The firmlinks (`/usr/share/firmlinks`: each root folder and its place on the data volume),
    /// `None` when they couldn't be read, so every `/System/Volumes/Data` path is refused.
    pub(crate) firmlinks: Option<Vec<(String, String)>>,
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
            remote_drive: remote_drive(),
            macos: cfg!(target_os = "macos"),
            firmlinks: {
                #[cfg(target_os = "macos")]
                let firmlinks = firmlinks();
                #[cfg(not(target_os = "macos"))]
                let firmlinks = Some(Vec::new());
                firmlinks
            },
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
    let rewritten;
    let path = if net.macos {
        match macos_rules::firmlinked(&text, net) {
            macos_rules::Mac::Refused => return true,
            macos_rules::Mac::Rewritten(form) => {
                rewritten = PathBuf::from(form);
                rewritten.as_path()
            }
            macos_rules::Mac::AsIs => path,
        }
    } else {
        path
    };
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix('/') {
        let first = OsStr::new(rest.split('/').next().unwrap_or_default());
        if name_folds_to(first, "net") || name_folds_to(first, "network") {
            return true;
        }
    }
    net.remote_mounts.iter().any(|mount| under(path, mount))
}

/// A mount name as a network check compares it: [`fold_name`] (which normalises to NFD, as
/// APFS stores names). On every system: a network check refuses, and over-matching is its safe
/// side.
fn name_key(name: &str) -> String {
    fold_name(name)
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

/// `name` folded for a match that refuses against a name that is not ASCII (the data folder's
/// path, a mount point): each code point stage C's table folds to an ASCII letter made that
/// letter ([`name_folds_to`]'s table: ASCII case, `ſ`, the Kelvin sign, `ı`, `İ`), then
/// normalised to NFD (as APFS stores names) and lowercased (`char::to_lowercase`, every other
/// cased letter). Against an ASCII name use [`name_folds_to`]. Over-matching is the safe side
/// for a deny; never use it for a match that allows ([`under_share_matter`] compares strictly).
pub(crate) fn fold_name(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| ascii_fold::ascii_letter(c).unwrap_or(c))
        .collect();
    mapped.nfd().flat_map(char::to_lowercase).collect()
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
        .and_then(linux::remote_mounts)
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

/// The firmlinks, read once and kept (macOS).
#[cfg(target_os = "macos")]
fn firmlinks() -> Option<Vec<(String, String)>> {
    static FIRMLINKS: std::sync::OnceLock<Option<Vec<(String, String)>>> =
        std::sync::OnceLock::new();
    FIRMLINKS
        .get_or_init(|| match std::fs::read_to_string("/usr/share/firmlinks") {
            Ok(text) => Some(macos_rules::parse_firmlinks(&text)),
            Err(error) => {
                tracing::warn!(%error, "the firmlinks could not be read; the data volume's paths are refused");
                None
            }
        })
        .clone()
}

/// This system's drive answer: on Windows each letter's device asked once (`QueryDosDeviceW`)
/// and kept for this `NetEnv`; no drive is remote elsewhere.
fn remote_drive() -> Box<dyn Fn(char) -> bool + Send> {
    #[cfg(windows)]
    {
        let known = std::sync::Mutex::new([None::<bool>; 26]);
        Box::new(move |letter: char| {
            let index = usize::from(u8::try_from(letter).unwrap_or(b'A').saturating_sub(b'A'));
            let Ok(mut known) = known.lock() else {
                return true;
            };
            match known.get_mut(index) {
                Some(Some(remote)) => *remote,
                Some(slot) => *slot.insert(windows::remote_drive(letter)),
                None => true,
            }
        })
    }
    #[cfg(not(windows))]
    {
        Box::new(|_| false)
    }
}
