//! The envelope values (decision record 0036, "Envelope values"): what each day report and
//! tally says about this build and machine.

use std::path::Path;

use clauseo_counts::About;
use clauseo_counts::schema::{Arch, Channel, InstallType, Language, Platform, Version};
use clauseo_settings::{Effective, KnownFolder, PlatformDirs, SystemDirs};

/// The release channel, a build constant.
const CHANNEL: Channel = Channel::Stable;

/// Windows 11's first build: an earlier build is Windows 10.
const WINDOWS_11_BUILD: u64 = 22_000;

/// The envelope for an app whose executable is `exe`, with the effective settings: desktop's
/// version, the channel, the OS and its major version, the CPU, `ui.language`, per machine or
/// per user, and whether any policy source was read. `None` when the schema has no value for this
/// version, OS version or CPU.
pub fn about(effective: &Effective, exe: Option<&Path>) -> Option<About> {
    let platform = platform_of(
        cfg!(target_os = "macos"),
        cfg!(windows),
        &tauri_plugin_os::version(),
    );
    if platform.is_none() {
        tracing::debug!("no usage counts: the schema has no value for this OS version");
    }
    Some(About {
        version: version()?,
        channel: CHANNEL,
        platform: platform?,
        arch: arch()?,
        language: language(effective.settings().ui.language),
        install_type: install_type(cfg!(windows), &SystemDirs, exe),
        managed: effective.policy_present(),
    })
}

/// Desktop's `CARGO_PKG_VERSION`, which `tests/version.rs` keeps plain `X.Y.Z`; none when a
/// part is over 999.
fn version() -> Option<Version> {
    env!("CARGO_PKG_VERSION").parse::<Version>().ok()
}

/// This OS and its major version, from the version the OS reports: macOS's major; Windows 11
/// from build 22000, else 10; Linux with none.
fn platform_of(macos: bool, windows: bool, version: &tauri_plugin_os::Version) -> Option<Platform> {
    let &tauri_plugin_os::Version::Semantic(major, _, build) = version else {
        return (!macos && !windows).then_some(Platform::Linux);
    };
    if macos {
        Platform::macos(u8::try_from(major).ok()?)
    } else if windows {
        Platform::windows(if build >= WINDOWS_11_BUILD { 11 } else { 10 })
    } else {
        Some(Platform::Linux)
    }
}

/// The CPU this build is for.
const fn arch() -> Option<Arch> {
    if cfg!(target_arch = "x86_64") {
        Some(Arch::X86_64)
    } else if cfg!(target_arch = "aarch64") {
        Some(Arch::Aarch64)
    } else {
        None
    }
}

fn language(language: clauseo_settings::Language) -> Language {
    match language {
        clauseo_settings::Language::En => Language::En,
        clauseo_settings::Language::Hi => Language::Hi,
    }
}

/// Per machine when, on Windows, the executable sits under the Program Files known folder;
/// per user otherwise.
fn install_type(windows: bool, dirs: &dyn PlatformDirs, exe: Option<&Path>) -> InstallType {
    let program_files = dirs.known_folder(KnownFolder::ProgramFiles);
    match (windows, exe, program_files) {
        (true, Some(exe), Some(folder)) if is_under(exe, &folder) => InstallType::PerMachine,
        _ => InstallType::PerUser,
    }
}

/// Whether `path` is inside `folder`, comparing each part without regard to case, as Windows
/// names files.
fn is_under(path: &Path, folder: &Path) -> bool {
    let mut parts = path.components();
    let mut inside = false;
    for want in folder.components() {
        inside = parts.next().is_some_and(|part| {
            part.as_os_str().to_string_lossy().to_lowercase()
                == want.as_os_str().to_string_lossy().to_lowercase()
        });
        if !inside {
            return false;
        }
    }
    inside && parts.next().is_some()
}

#[cfg(test)]
mod tests;
