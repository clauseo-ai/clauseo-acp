//! Where Clauseo keeps its data and puts new matters: [`Locations`], and the [`PlatformDirs`]
//! and [`Env`] it reads them from.

#[cfg(test)]
mod tests;

use crate::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The app's identifier. It names the data folder and the macOS managed preferences domain,
/// and matches the identifier in the desktop crate's `tauri.conf.json`.
pub const IDENTIFIER: &str = "ai.clauseo.desktop";

/// The variable that moves the data folder, for `cli` and CI. It never moves a policy path.
pub const DATA_DIR_VAR: &str = "CLAUSEO_DATA_DIR";

/// The name of the user's settings file, in the data folder.
pub const SETTINGS_FILE: &str = "settings.json";

/// The folders Clauseo uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    data_dir: PathBuf,
    settings_file: PathBuf,
    default_matters_dir: PathBuf,
}

/// The OS's folders. [`crate::SystemDirs`] asks the OS; tests pass their own.
pub trait PlatformDirs {
    /// The user's home folder.
    fn home(&self) -> Option<PathBuf>;
    /// Windows' per-user local (never roaming) app data folder. Not used on other OSes.
    fn local_data(&self) -> Option<PathBuf>;
    /// Windows' Documents folder, wherever Known Folder Move put it. Not used on other OSes.
    fn documents(&self) -> Option<PathBuf>;
    /// A Windows known folder. `None` on other OSes, and for a folder this machine lacks.
    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf>;
}

/// The Windows known folders [`crate::refused_folders`] and the policy file need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KnownFolder {
    /// `%APPDATA%`, the roaming app data folder.
    RoamingAppData,
    /// `%LOCALAPPDATA%`.
    LocalAppData,
    /// `%SystemRoot%`.
    Windows,
    /// `Program Files`.
    ProgramFiles,
    /// `Program Files (x86)`.
    ProgramFilesX86,
    /// `Program Files (Arm)`, beside `Program Files`, on the machines that have it.
    ProgramFilesArm,
    /// `ProgramData`.
    ProgramData,
    /// `Public`.
    Public,
}

/// The environment. [`crate::SystemEnv`] reads the process's; tests pass their own.
pub trait Env {
    /// The variable's value, or `None` when it is not set.
    fn var(&self, name: &str) -> Option<OsString>;
}

/// The OS whose folder rules apply. Every rule is a plain function of this, so each OS's rules
/// are tested on every OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Os {
    Windows,
    Mac,
    /// Linux and the other Unix-likes, with the XDG folders.
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub(crate) const CURRENT: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
}

impl Locations {
    /// The folders on this OS. `CLAUSEO_DATA_DIR`, when set, replaces the data folder.
    pub fn resolve(dirs: &dyn PlatformDirs, env: &dyn Env) -> Result<Locations, Error> {
        Self::resolve_for(Os::CURRENT, dirs, env)
    }

    /// The folders under `root`: the data folder is `root`, and new matters go in
    /// `root/Documents/Clauseo`. Reads nothing; for tests and `cli`.
    pub fn under_root(root: &Path) -> Locations {
        Self::with_data_dir(root.to_owned(), root.join("Documents").join("Clauseo"))
    }

    /// The data folder, which holds `settings.json` and the matters' history.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The user's `settings.json`.
    pub fn settings_file(&self) -> &Path {
        &self.settings_file
    }

    /// Where new matters go when `matters.default_location` is not set: `Documents/Clauseo`.
    /// `host` adds each matter's name.
    pub fn default_matters_dir(&self) -> &Path {
        &self.default_matters_dir
    }

    pub(crate) fn resolve_for(
        os: Os,
        dirs: &dyn PlatformDirs,
        env: &dyn Env,
    ) -> Result<Locations, Error> {
        let home = || dirs.home().ok_or(Error::NoFolder { folder: "home" });
        let data_dir = match env.var(DATA_DIR_VAR).filter(|value| !value.is_empty()) {
            Some(value) => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(Error::RelativeDataDir { path });
                }
                path
            }
            None => match os {
                Os::Windows => dirs
                    .local_data()
                    .ok_or(Error::NoFolder {
                        folder: "LocalAppData",
                    })?
                    .join(IDENTIFIER),
                Os::Mac => home()?
                    .join("Library")
                    .join("Application Support")
                    .join(IDENTIFIER),
                Os::Linux => match absolute_var(env, "XDG_DATA_HOME") {
                    Some(data_home) => data_home,
                    None => home()?.join(".local").join("share"),
                }
                .join(IDENTIFIER),
            },
        };
        let documents = match os {
            Os::Windows => dirs.documents().ok_or(Error::NoFolder {
                folder: "Documents",
            })?,
            Os::Mac => home()?.join("Documents"),
            Os::Linux => match absolute_var(env, "XDG_DOCUMENTS_DIR") {
                Some(documents) => documents,
                None => home()?.join("Documents"),
            },
        };
        Ok(Self::with_data_dir(data_dir, documents.join("Clauseo")))
    }

    fn with_data_dir(data_dir: PathBuf, default_matters_dir: PathBuf) -> Locations {
        Locations {
            settings_file: data_dir.join(SETTINGS_FILE),
            data_dir,
            default_matters_dir,
        }
    }
}

/// A variable that holds an absolute path. The XDG rules say to ignore any other value.
fn absolute_var(env: &dyn Env, name: &str) -> Option<PathBuf> {
    env.var(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
