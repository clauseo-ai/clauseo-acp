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

/// The log folder's name under the data folder ([`Locations::log_dir`]).
const LOGS_DIR: &str = "logs";

/// The variable that moves the data folder, for `cli`, CI and by-hand runs. It never moves a
/// policy path. With it set, the default matters folder moves beside the data folder too (see
/// [`Locations::default_matters_dir`]), so a run with a scratch data folder creates its matters
/// there and not in the user's Documents.
pub const DATA_DIR_VAR: &str = "CLAUSEO_DATA_DIR";

/// The name of the user's settings file, in the data folder.
pub const SETTINGS_FILE: &str = "settings.json";

/// The name of the updater's marker file, in the data folder ([`Locations::update_marker`]).
const UPDATE_MARKER: &str = "update.json";

/// The name of the updater's download folder, in the data folder ([`Locations::updates_dir`]).
const UPDATES_DIR: &str = "updates";

/// The agents' folder under the data folder, which holds each [`Locations::agent_home`].
const AGENTS_DIR: &str = "agents";

/// Every entry of the data folder this file names: the log folder, the settings file, the
/// updater's marker and download folder, and the agents' folder. The host's data folder fence
/// checks that each is denied to agents or allowed for a stated reason, so a renamed one fails
/// its guard test until the fence names it; and a test here checks that every entry `Locations`
/// builds under the data folder (each `data_dir.join`) is listed, so a new one fails too.
pub const DATA_ENTRIES: [&str; 5] = [
    LOGS_DIR,
    SETTINGS_FILE,
    UPDATE_MARKER,
    UPDATES_DIR,
    AGENTS_DIR,
];

/// The folders Clauseo uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    data_dir: PathBuf,
    settings_file: PathBuf,
    default_matters_dir: Option<PathBuf>,
}

/// The OS's folders. [`crate::SystemDirs`] asks the OS; tests pass their own. Off Windows only
/// [`PlatformDirs::home`] is asked, so a fake for macOS or Linux can return `None` from
/// [`PlatformDirs::known_folder`].
pub trait PlatformDirs {
    /// The user's home folder.
    fn home(&self) -> Option<PathBuf>;
    /// A Windows known folder. `None` on other OSes, and for a folder this machine lacks.
    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf>;
}

/// The Windows known folders [`Locations::resolve`], [`crate::refused_folders`] and the policy
/// file need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KnownFolder {
    /// `%APPDATA%`, the roaming app data folder.
    RoamingAppData,
    /// `%LOCALAPPDATA%`, the per-user local (never roaming) app data folder: the data folder's
    /// parent.
    LocalAppData,
    /// The user's Documents folder, wherever Known Folder Move put it: new matters go in it.
    /// Windows may not give it (a redirect to a share it cannot reach, a deleted folder); then
    /// there is no default matters folder, and nothing else needs it.
    Documents,
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
        Self::with_data_dir(
            root.to_owned(),
            Some(root.join("Documents").join("Clauseo")),
        )
    }

    /// The folders of a home at `root`, as a real one lays them out: the data folder is
    /// `root/data` and new matters go in `root/Documents/Clauseo`, outside the data folder (a
    /// matter inside it is refused). Reads nothing; for tests and `cli`.
    pub fn under_home(root: &Path) -> Locations {
        Self::with_data_dir(
            root.join("data"),
            Some(root.join("Documents").join("Clauseo")),
        )
    }

    /// These folders with no default matters folder, as on a machine with no Documents folder
    /// (issue #365). Reads nothing; for tests.
    pub fn without_default_matters_dir(self) -> Locations {
        Locations {
            default_matters_dir: None,
            ..self
        }
    }

    /// The data folder, which holds `settings.json` and the matters' history.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The user's `settings.json`.
    pub fn settings_file(&self) -> &Path {
        &self.settings_file
    }

    /// Where new matters go when `matters.default_location` is not set. Without
    /// `CLAUSEO_DATA_DIR`, that is `Documents/Clauseo`. With it, it is `<data dir>-matters`, a
    /// sibling of the data folder (same parent, the data folder's name plus `-matters`): a matter
    /// inside the data folder is always refused, since the matters' refused trees hold the whole
    /// data folder, and the matters root is `<data dir>/matters`. A data folder with no name (a
    /// root such as `/`) has no sibling, so it keeps `Documents/Clauseo`. `host` adds each
    /// matter's name.
    ///
    /// `None` when the Documents folder is needed and the OS gives none (issue #365): Windows
    /// cannot resolve it (a redirect to a share it cannot reach, a deleted folder), or macOS or
    /// Linux has no home. Then New matter asks for a folder. Only the data folder is essential.
    pub fn default_matters_dir(&self) -> Option<&Path> {
        self.default_matters_dir.as_deref()
    }

    /// The folder of the host's log file, `<data folder>/logs` (decision record 0047): it
    /// follows `CLAUSEO_DATA_DIR`, so a test's log stays in its own data folder. Reads nothing
    /// and makes nothing; the host makes it when it installs the log.
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join(LOGS_DIR)
    }

    /// The home Clauseo owns for agent `agent`: `<data folder>/agents/<agent>/home`. The host
    /// makes it at launch and runs Codex with it as `CODEX_HOME`, so Codex keeps its login
    /// there and never in the user's `~/.codex` (#160); the limits reader reads the same one.
    /// `agent` is an agent id's text (`[a-z0-9-]`, as `protocol`'s `AgentId` checks), so it is
    /// one plain folder name. It is under the data folder, never the temp folder: a temp
    /// cleaner would sign the lawyer out.
    pub fn agent_home(&self, agent: &str) -> PathBuf {
        // A free-text caller must not put the home, and the login with it, outside
        // `<data>/agents`.
        debug_assert!(
            !agent.is_empty()
                && agent
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "an agent id is [a-z0-9-]+"
        );
        self.data_dir.join(AGENTS_DIR).join(agent).join("home")
    }

    /// The updater's marker file, `<data folder>/update.json` (task R23): the host writes there
    /// the update it is about to install, and reads it at the next start to tell an install
    /// that took from one that failed. In the data folder, so `CLAUSEO_DATA_DIR` moves it, and
    /// never in a matters folder.
    pub fn update_marker(&self) -> PathBuf {
        self.data_dir.join(UPDATE_MARKER)
    }

    /// Where the updater keeps its downloads, `<data folder>/updates` (task R23), one folder per
    /// version. In the data folder, never the temp folder or a matters folder, so a download
    /// kept for the next start survives a temp cleaner and never shows among a lawyer's files.
    pub fn updates_dir(&self) -> PathBuf {
        self.data_dir.join(UPDATES_DIR)
    }

    pub(crate) fn resolve_for(
        os: Os,
        dirs: &dyn PlatformDirs,
        env: &dyn Env,
    ) -> Result<Locations, Error> {
        let home = || dirs.home().ok_or(Error::NoFolder { folder: "home" });
        let overridden = match env.var(DATA_DIR_VAR).filter(|value| !value.is_empty()) {
            Some(value) => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(Error::RelativeDataDir { path });
                }
                Some(path)
            }
            None => None,
        };
        let data_dir = match overridden.clone() {
            Some(path) => path,
            None => match os {
                Os::Windows => dirs
                    .known_folder(KnownFolder::LocalAppData)
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
        let default_matters_dir = match overridden.as_deref().and_then(matters_beside) {
            Some(beside) => Some(beside),
            None => documents(os, dirs, env).map(|documents| documents.join("Clauseo")),
        };
        Ok(Self::with_data_dir(data_dir, default_matters_dir))
    }

    fn with_data_dir(data_dir: PathBuf, default_matters_dir: Option<PathBuf>) -> Locations {
        Locations {
            settings_file: data_dir.join(SETTINGS_FILE),
            data_dir,
            default_matters_dir,
        }
    }
}

/// The user's Documents folder, or `None`, logged, when the OS gives none: never an error, since
/// only the default matters folder needs it (issue #365).
fn documents(os: Os, dirs: &dyn PlatformDirs, env: &dyn Env) -> Option<PathBuf> {
    let documents = match os {
        Os::Windows => dirs.known_folder(KnownFolder::Documents),
        Os::Mac => dirs.home().map(|home| home.join("Documents")),
        Os::Linux => absolute_var(env, "XDG_DOCUMENTS_DIR")
            .or_else(|| dirs.home().map(|home| home.join("Documents"))),
    };
    if documents.is_none() {
        tracing::warn!(
            ?os,
            "no Documents folder: there is no default matters folder, so New matter asks for one"
        );
    }
    documents
}

/// The default matters folder beside a data folder that `CLAUSEO_DATA_DIR` named:
/// `<data dir>-matters`, in the same parent. `None` when the data folder has no name (a root), so
/// the caller keeps `Documents/Clauseo`.
fn matters_beside(data_dir: &Path) -> Option<PathBuf> {
    let mut sibling = data_dir.file_name()?.to_owned();
    sibling.push("-matters");
    Some(data_dir.with_file_name(sibling))
}

/// A variable that holds an absolute path. The XDG rules say to ignore any other value.
fn absolute_var(env: &dyn Env, name: &str) -> Option<PathBuf> {
    env.var(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
