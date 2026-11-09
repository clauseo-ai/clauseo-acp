//! The file-system grants the browser's package needs, named and never applied: the owner
//! applies them where it applies the browser account's (set-up), and removes them there.

use std::path::{Component, Path, PathBuf};

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
/// `pins` is the `browser-pins` folder; a folder that is it, holds it or sits inside it is
/// refused, since the inherited ACE would reach it. Paths compare by their parts, ASCII letters
/// in either case, as given: the caller passes the paths it will apply the ACEs to.
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
    let pins = parts(pins);
    let mut out = Vec::with_capacity(named.len());
    for (path, rights) in named {
        if !path.is_absolute() {
            return Err(Error::Grant {
                reason: "a browser folder is not an absolute path",
            });
        }
        let folder = parts(path);
        if folder.starts_with(&pins) || pins.starts_with(&folder) {
            return Err(Error::Grant {
                reason: "a browser folder is, holds or sits in browser-pins",
            });
        }
        out.push(Grant {
            path: path.clone(),
            rights,
            sid: sid.clone(),
        });
    }
    Ok(out)
}

/// The path's parts, ASCII-lowercased, without `.` parts and trailing separators.
fn parts(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root that is absolute on the OS running the test.
    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\ProgramData\Clauseo")
        } else {
            PathBuf::from("/var/clauseo")
        }
    }

    fn folders() -> BrowserFolders {
        let r = root();
        BrowserFolders {
            profile: r.join("browser").join("profile"),
            staging: r.join("browser").join("staging"),
            tmp: r.join("browser").join("tmp"),
            shell: r.join("shells").join("157"),
        }
    }

    fn package() -> PackageName {
        PackageName::new("Clauseo.Browser.B01").unwrap()
    }

    #[test]
    fn the_four_folders_get_their_rights_for_the_package() {
        let f = folders();
        let list = grants(&package(), &f, &root().join("browser-pins")).unwrap();
        let sid = package_sid(&package());
        let got: Vec<(&Path, Rights)> = list.iter().map(|g| (g.path.as_path(), g.rights)).collect();
        assert_eq!(
            got,
            [
                (f.shell.as_path(), Rights::ReadExecute),
                (f.profile.as_path(), Rights::ReadWrite),
                (f.staging.as_path(), Rights::ReadWrite),
                (f.tmp.as_path(), Rights::ReadWrite),
            ]
        );
        assert!(list.iter().all(|g| g.sid == sid));
    }

    #[test]
    fn the_masks_are_icacls_m_and_rx() {
        assert_eq!(Rights::ReadWrite.mask(), 0x0013_01BF);
        assert_eq!(Rights::ReadExecute.mask(), 0x0012_00A9);
    }

    #[test]
    fn a_folder_over_under_or_at_the_pins_is_refused() {
        let pins = root().join("browser").join("Browser-Pins");
        let mut over = folders();
        over.profile = root().join("browser");
        let mut under = folders();
        under.staging = pins.join("x");
        let mut at = folders();
        at.tmp = root().join("browser").join("browser-pins").join(".");
        for (f, what) in [(over, "holds"), (under, "inside"), (at, "same")] {
            let refused = grants(&package(), &f, &pins);
            assert!(
                matches!(refused, Err(Error::Grant { .. })),
                "{what}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_sibling_named_like_the_pins_is_allowed() {
        let mut f = folders();
        f.tmp = root().join("browser").join("browser-pins-tmp");
        assert!(grants(&package(), &f, &root().join("browser").join("browser-pins")).is_ok());
    }

    #[test]
    fn a_relative_folder_is_refused() {
        let mut f = folders();
        f.tmp = PathBuf::from("tmp");
        assert!(grants(&package(), &f, &root().join("browser-pins")).is_err());
    }
}
