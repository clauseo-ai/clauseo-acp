//! The OS behind the traits: [`SystemDirs`] and [`SystemEnv`]. Tests never use them, except
//! the read-only tests of the OS shims.

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use crate::locations::{Env, KnownFolder, PlatformDirs};
use std::ffi::OsString;
use std::path::PathBuf;

/// [`PlatformDirs`] from the OS: the known-folder API on Windows, so a Documents folder that
/// Known Folder Move put in a cloud drive resolves to the real folder; the home folder elsewhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDirs;

/// [`Env`] over the process's environment: the only place this crate reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

#[cfg(windows)]
impl PlatformDirs for SystemDirs {
    fn home(&self) -> Option<PathBuf> {
        windows::known_folder(&::windows::Win32::UI::Shell::FOLDERID_Profile)
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        windows::folder(folder)
    }
}

#[cfg(not(windows))]
impl PlatformDirs for SystemDirs {
    fn home(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }

    fn known_folder(&self, _: KnownFolder) -> Option<PathBuf> {
        None
    }
}
