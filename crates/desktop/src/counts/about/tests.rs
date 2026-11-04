//! The envelope values desktop fills in.

use std::path::{Path, PathBuf};

use clauseo_counts::schema::{InstallType, Platform};
use clauseo_settings::{KnownFolder, PlatformDirs};
use tauri_plugin_os::Version;

use super::*;

#[test]
fn the_os_major_is_macos_major_or_windows_by_build() {
    let mac = |v| platform_of(true, false, &v);
    assert_eq!(mac(Version::Semantic(15, 3, 1)), Platform::macos(15));
    assert_eq!(mac(Version::Semantic(13, 0, 0)), Platform::macos(13));
    // A version the schema has no value for: no platform, so no counts.
    assert_eq!(mac(Version::Semantic(12, 7, 0)), None);
    assert_eq!(mac(Version::Unknown), None);

    let win = |v| platform_of(false, true, &v);
    assert_eq!(win(Version::Semantic(10, 0, 22_000)), Platform::windows(11));
    assert_eq!(win(Version::Semantic(10, 0, 26_100)), Platform::windows(11));
    assert_eq!(win(Version::Semantic(10, 0, 21_999)), Platform::windows(10));
    assert_eq!(win(Version::Semantic(10, 0, 19_045)), Platform::windows(10));
    assert_eq!(win(Version::Unknown), None);

    let linux = |v| platform_of(false, false, &v);
    assert_eq!(linux(Version::Semantic(6, 8, 0)), Some(Platform::Linux));
    assert_eq!(linux(Version::Rolling(None)), Some(Platform::Linux));
}

/// Program Files at `C:\\Program Files`, as Windows names it, with `/` for this OS's paths.
struct Dirs(PathBuf);

impl PlatformDirs for Dirs {
    fn home(&self) -> Option<PathBuf> {
        None
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        (folder == KnownFolder::ProgramFiles).then(|| self.0.clone())
    }
}

#[test]
fn per_machine_only_for_a_windows_exe_under_program_files() {
    let dirs = Dirs(PathBuf::from("/drive/Program Files"));
    let exe = |p: &str| Some(PathBuf::from(p));
    let kind = |windows, exe: Option<PathBuf>| install_type(windows, &dirs, exe.as_deref());
    assert_eq!(
        kind(true, exe("/drive/Program Files/Clauseo/Clauseo.exe")),
        InstallType::PerMachine
    );
    assert_eq!(
        kind(true, exe("/DRIVE/program files/Clauseo/Clauseo.exe")),
        InstallType::PerMachine
    );
    for per_user in [
        "/drive/Users/a/AppData/Local/Clauseo/Clauseo.exe",
        "/drive/Program Files (x86)/Clauseo/Clauseo.exe",
        "/drive/Program",
        "/drive/Program Files",
    ] {
        assert_eq!(
            kind(true, exe(per_user)),
            InstallType::PerUser,
            "{per_user}"
        );
    }
    assert_eq!(kind(true, None), InstallType::PerUser);
    // Not Windows: per user wherever it is.
    assert_eq!(
        kind(false, exe("/drive/Program Files/Clauseo/Clauseo.exe")),
        InstallType::PerUser
    );
    assert!(!is_under(Path::new("/a"), Path::new("/a/b")));
}
