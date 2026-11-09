//! The file-system grants the browser's package needs, named and never applied: the owner
//! applies them where it applies the browser account's (set-up), and removes them there.

use std::path::{Component, Path, PathBuf, Prefix};

use crate::error::Error;
use crate::package::{PackageName, Sid, package_sid};

/// The browser's folders, as the owner lays them out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserFolders {
    /// The browser profile (`--user-data-dir`).
    pub profile: PathBuf,
    /// The downloads and screenshots staged for the owner to collect.
    pub staging: PathBuf,
    /// The browser's `TEMP` and `TMP`.
    pub tmp: PathBuf,
    /// The chrome-headless-shell install: run, never written.
    pub shell: PathBuf,
}

/// What a grant allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rights {
    /// Read, write, create and delete (icacls `M`): `FILE_GENERIC_READ`, `FILE_GENERIC_WRITE`,
    /// `FILE_GENERIC_EXECUTE` and `DELETE`, mask `0x1301BF`.
    ReadWrite,
    /// Read and run (icacls `RX`): `FILE_GENERIC_READ` and `FILE_GENERIC_EXECUTE`, mask
    /// `0x1200A9`.
    ReadExecute,
}

impl Rights {
    /// The access mask of the ACE.
    pub fn mask(self) -> u32 {
        match self {
            Rights::ReadWrite => 0x0013_01BF,
            Rights::ReadExecute => 0x0012_00A9,
        }
    }
}

/// One allow ACE to add on a folder, inherited by its files and folders (`(OI)(CI)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The folder.
    pub path: PathBuf,
    /// What it allows.
    pub rights: Rights,
    /// Whom it allows: the package SID. Never log it.
    pub sid: Sid,
}

/// The grants the package of `package` needs on `folders`: read and run on the shell, read and
/// write on the profile, the staging folder and the temp folder. Nothing else: the browser
/// account's own folders, `browser-pins` and every other path stay closed to the package.
///
/// `pins` is the `browser-pins` folder. The paths are compared by what they are, not how they
/// are written: each of the five must exist as a folder and is opened (`fs::canonicalize`, which
/// on Windows is `GetFinalPathNameByHandleW`), and its final path must be the path given, letter
/// case aside. So a path through a junction, a symbolic link, a short (8.3) name or a
/// substituted drive is refused, as is one with `..`, a `\\?\`, `\??\`, `\\.\` or network
/// prefix: the caller passes the plain final paths it will apply the ACEs to. No two of the
/// four folders may be one folder or hold one another (a profile holding the shell would get
/// write access to it), and none may be, hold or sit in `pins`, since the inherited ACE would
/// reach it.
pub fn grants(
    package: &PackageName,
    folders: &BrowserFolders,
    pins: &Path,
) -> Result<Vec<Grant>, Error> {
    let sid = package_sid(package);
    let named = [
        (&folders.shell, Rights::ReadExecute),
        (&folders.profile, Rights::ReadWrite),
        (&folders.staging, Rights::ReadWrite),
        (&folders.tmp, Rights::ReadWrite),
    ];
    let pins = final_parts(pins)?;
    let mut seen: Vec<Vec<String>> = Vec::with_capacity(named.len());
    let mut out = Vec::with_capacity(named.len());
    for (path, rights) in named {
        let folder = final_parts(path)?;
        if overlap(&folder, &pins) {
            return Err(Error::Grant {
                reason: "a browser folder is, holds or sits in browser-pins",
            });
        }
        if seen.iter().any(|other| overlap(&folder, other)) {
            return Err(Error::Grant {
                reason: "two browser folders are one folder, or one holds the other",
            });
        }
        seen.push(folder);
        out.push(Grant {
            path: path.clone(),
            rights,
            sid: sid.clone(),
        });
    }
    Ok(out)
}

/// Whether one of two paths' parts is the other's or starts it.
fn overlap(a: &[String], b: &[String]) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// The parts of `path`'s final path, after the checks [`grants`] lists.
fn final_parts(path: &Path) -> Result<Vec<String>, Error> {
    let refuse = |reason| Err(Error::Grant { reason });
    let text = path.as_os_str().to_string_lossy();
    if [r"\\?\", r"\??\", r"\\.\", "//?/", "//./"]
        .iter()
        .any(|p| text.starts_with(p))
    {
        return refuse("a browser folder has a verbatim, NT or device prefix");
    }
    if !path.is_absolute() {
        return refuse("a browser folder is not an absolute path");
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return refuse("a browser folder has a .. part");
    }
    if let Some(Component::Prefix(prefix)) = path.components().next()
        && !matches!(prefix.kind(), Prefix::Disk(_))
    {
        return refuse("a browser folder is not on a local drive");
    }
    let Ok(opened) = opened(path) else {
        return refuse("a browser folder or browser-pins cannot be opened");
    };
    if !opened.is_dir() {
        return refuse("a browser folder or browser-pins is not a folder");
    }
    let Some(shown) = without_verbatim(&opened) else {
        return refuse("a browser folder's final path is not on a local drive");
    };
    let (given, found) = (parts(path), parts(&shown));
    if given != found {
        return refuse(
            "a browser folder is reached through a junction, a link, a short (8.3) name or a \
             substituted drive; give its final path",
        );
    }
    Ok(found)
}

/// The final path of `path`: `fs::canonicalize`, which opens it and asks the OS
/// (`GetFinalPathNameByHandleW` on Windows, `realpath` elsewhere).
#[expect(
    clippy::disallowed_methods,
    reason = "the verbatim form is wanted: without_verbatim reads and strips it"
)]
pub(crate) fn opened(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// `fs::canonicalize`'s result as a plain path: on Windows `\\?\C:\…` becomes `C:\…`, and
/// anything else verbatim (`\\?\UNC\…`, a volume GUID) is `None`. Unchanged elsewhere.
pub(crate) fn without_verbatim(path: &Path) -> Option<PathBuf> {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => {
                let rest: PathBuf = path.components().skip(1).collect();
                let drive = format!("{}:\\", char::from(letter));
                Some(Path::new(&drive).join(rest.strip_prefix(r"\").unwrap_or(&rest)))
            }
            Prefix::Disk(_) => Some(path.to_owned()),
            _ => None,
        },
        _ => Some(path.to_owned()),
    }
}

/// The path's parts, lowercased (Unicode, as NTFS compares names), without `.` parts and
/// trailing separators.
fn parts(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests;
