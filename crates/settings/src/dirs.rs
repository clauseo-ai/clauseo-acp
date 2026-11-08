//! The folders a lawyer may not pick as a matter: [`refused_folders`].

#[cfg(test)]
mod tests;

use crate::locations::{KnownFolder, Os, PlatformDirs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Folders inside home that hold app data and keys, on every OS. `.codex` holds Codex's logins
/// and settings (R17 PR-E).
const HOME_DOT_FOLDERS: [&str; 7] = [
    ".config", ".local", ".cache", ".ssh", ".gnupg", ".aws", ".codex",
];

/// macOS system folders.
const MACOS_SYSTEM: [&str; 12] = [
    "/System",
    "/Library",
    "/Applications",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/private",
    "/opt",
    "/Volumes",
    "/cores",
    "/dev",
];

/// Linux system folders.
const LINUX_SYSTEM: [&str; 18] = [
    "/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/boot", "/proc", "/sys", "/dev", "/run",
    "/var", "/opt", "/srv", "/root", "/snap", "/mnt", "/media",
];

/// Windows known folders that are system or app data folders.
const WINDOWS_KNOWN: [KnownFolder; 8] = [
    KnownFolder::RoamingAppData,
    KnownFolder::LocalAppData,
    KnownFolder::Windows,
    KnownFolder::ProgramFiles,
    KnownFolder::ProgramFilesX86,
    KnownFolder::ProgramFilesArm,
    KnownFolder::ProgramData,
    KnownFolder::Public,
];

/// Folders at the root of the Windows system drive.
const WINDOWS_DRIVE_ROOT: [&str; 2] = ["$Recycle.Bin", "System Volume Information"];

/// The folders refused as a matter folder with everything inside them (`matters`' refused
/// trees), as written: `~/.codex` and the folder `CODEX_HOME` names, which hold Codex's logins
/// and settings (R17 PR-E). [`refused_folders`] lists them too, so a folder holding one is
/// refused as well; `host` adds the data folder. `resolve` is the [`Resolve`] the `CODEX_HOME`
/// guard compares with.
pub fn refused_trees(dirs: &dyn PlatformDirs, resolve: Resolve<'_>) -> Vec<PathBuf> {
    let mut trees: Vec<PathBuf> = dirs
        .home()
        .map(|home| home.join(".codex"))
        .into_iter()
        .collect();
    trees.extend(codex_home(dirs, resolve));
    trees
}

/// A folder's real path, which the `CODEX_HOME` guard compares: symlinks and `..` followed, a
/// Windows short name made long, and a part not made yet appended as written; `None` when it
/// can't be resolved. The caller gives it (desktop resolves with `dunce`), so this crate reads
/// no disk for it; a test gives a fake.
pub type Resolve<'a> = &'a dyn Fn(&Path) -> Option<PathBuf>;

/// The folder `CODEX_HOME` names, unless it is the home folder, a folder above it, or a root
/// (`/`, a drive's `C:\`): refusing that tree would refuse every matter, so it counts as not
/// set, with one warning per process that names no path. Compared on paths `resolve` resolved,
/// as `matters` compares folders (a symlink, `..`, a Windows short name), and without case on
/// Windows and macOS; a `CODEX_HOME` that does not resolve counts as not set too. Kept as
/// written when it counts.
fn codex_home(dirs: &dyn PlatformDirs, resolve: Resolve<'_>) -> Option<PathBuf> {
    static WARNED: AtomicBool = AtomicBool::new(false);
    let codex = dirs.codex_home()?;
    let (ignored, why) = match resolve(&codex) {
        None => (true, "does not resolve"),
        Some(real) if real.parent().is_none() => (true, "a root"),
        Some(real) => {
            let home = dirs.home().map(|home| resolve(&home).unwrap_or(home));
            let over = home.is_some_and(|home| folded(&home).starts_with(folded(&real)));
            (over, "the home folder or a folder above it")
        }
    };
    if !ignored {
        return Some(codex);
    }
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(why, "CODEX_HOME is ignored");
    }
    None
}

/// `path` as compared with the home folder: lowercased on Windows and macOS, whose folders
/// ignore case by default.
fn folded(path: &Path) -> PathBuf {
    if cfg!(any(windows, target_os = "macos")) {
        PathBuf::from(path.as_os_str().to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// The home folder, the folders inside it that hold app data and keys (`~/.codex` too, and the
/// folder `CODEX_HOME` names), and the OS's system folders, as written (not canonicalised).
/// `host` adds the data folder, and `matters` refuses drive roots, each listed folder and any
/// folder above one. This is a sanity check on import, not a security boundary. `resolve` is the
/// [`Resolve`] the `CODEX_HOME` guard compares with.
pub fn refused_folders(dirs: &dyn PlatformDirs, resolve: Resolve<'_>) -> Vec<PathBuf> {
    refused_for(Os::CURRENT, dirs, resolve)
}

pub(crate) fn refused_for(os: Os, dirs: &dyn PlatformDirs, resolve: Resolve<'_>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let home = dirs.home();
    if let Some(home) = &home {
        folders.push(home.clone());
        folders.extend(HOME_DOT_FOLDERS.iter().map(|name| home.join(name)));
    }
    // The lawyer's Codex folder where `CODEX_HOME` moved it: refused like `~/.codex`.
    folders.extend(codex_home(dirs, resolve));
    match os {
        Os::Mac => {
            folders.extend(home.iter().map(|home| home.join("Library")));
            folders.extend(MACOS_SYSTEM.iter().map(PathBuf::from));
        }
        Os::Linux => folders.extend(LINUX_SYSTEM.iter().map(PathBuf::from)),
        Os::Windows => {
            folders.extend(
                WINDOWS_KNOWN
                    .iter()
                    .filter_map(|&folder| dirs.known_folder(folder)),
            );
            folders.extend(
                home.as_deref()
                    .and_then(Path::parent)
                    .map(|users| users.join("Default")),
            );
            let drive_root = dirs
                .known_folder(KnownFolder::Windows)
                .and_then(|windows| windows.ancestors().last().map(Path::to_owned));
            if let Some(root) = drive_root {
                folders.extend(WINDOWS_DRIVE_ROOT.iter().map(|name| root.join(name)));
            }
        }
    }
    folders
}
