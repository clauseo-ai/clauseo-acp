//! How this copy of the app was installed (decision record 0037), which decides whether it
//! updates itself. Found once, at start ([`detect`]); the decisions are plain functions over
//! what was read, so tests need no registry and no `.app`.
//!
//! - **Windows:** per machine when `Clauseo.exe` is under the Program Files known folder (the
//!   rule the usage counts' `install_type` uses), or the machine-wide uninstall key
//!   `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo` exists (64-bit view),
//!   which the per-machine installer writes. Otherwise per user.
//! - **macOS:** a read-only location when the user cannot write the folder holding `Clauseo.app`
//!   or the `.app` itself (`access(W_OK)`): the plugin moves the old bundle out of its folder,
//!   which needs both, and would otherwise ask for an administrator. Otherwise per user.
//! - **Linux:** always a package (its host never gets this updater anyway).

#[cfg(windows)]
mod windows;

#[cfg(any(test, windows))]
use std::io;
#[cfg(any(test, target_os = "macos"))]
use std::path::Path;

use clauseo_host::InstallKind;

/// How the running app was installed.
pub(crate) fn detect() -> InstallKind {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().ok();
        let under = crate::counts::under_program_files(exe.as_deref());
        windows_kind(under, windows::uninstall_key_exists())
    }
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok();
        mac_kind(exe.as_deref(), writable)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        InstallKind::Package
    }
}

/// Windows: per machine when the exe is `under_program_files`, or the machine-wide uninstall
/// key exists. A key that could not be read counts as there (logged by its kind): updating a
/// per-machine install from the per-user installer would leave two copies, while a per-user
/// one that does not update shows who manages it.
#[cfg(any(test, windows))]
pub(super) fn windows_kind(
    under_program_files: bool,
    uninstall_key: io::Result<bool>,
) -> InstallKind {
    let machine_key = uninstall_key.unwrap_or_else(|error| {
        tracing::warn!(kind = %error.kind(), "the machine's uninstall key was not read");
        true
    });
    if under_program_files || machine_key {
        InstallKind::PerMachine
    } else {
        InstallKind::CurrentUser
    }
}

/// macOS: per user when `writable` says both the `.app` that `exe` runs from and the folder
/// holding it can be written; a read-only location otherwise, or when `exe` is not inside an
/// `.app` (nothing the plugin could swap). Moving a directory to another folder needs write
/// permission on the directory itself, so a root-owned `.app` (an MDM install) in an
/// admin-writable `/Applications` is read-only to its user.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn mac_kind(exe: Option<&Path>, writable: impl Fn(&Path) -> bool) -> InstallKind {
    match exe.and_then(bundle) {
        Some(app) if app.parent().is_some_and(&writable) && writable(app) => {
            InstallKind::CurrentUser
        }
        _ => InstallKind::ReadOnlyLocation,
    }
}

/// The folder holding the bundle `exe` runs from: `/Applications` for
/// `/Applications/Clauseo.app/Contents/MacOS/Clauseo`. None when `exe` is not inside an `.app`.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn bundle_folder(exe: &Path) -> Option<&Path> {
    bundle(exe)?.parent()
}

/// The bundle `exe` runs from: `/Applications/Clauseo.app` for
/// `/Applications/Clauseo.app/Contents/MacOS/Clauseo`. None when `exe` is not inside an `.app`.
#[cfg(any(test, target_os = "macos"))]
fn bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    is_bundle.then_some(app)
}

/// Whether this user may write `folder` (`access(2)` with `W_OK`, which asks the kernel, so ACLs
/// and read-only volumes count).
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "FFI to access(2)")]
fn writable(folder: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(path) = std::ffi::CString::new(folder.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a live NUL-terminated string for the call; access reads it only.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}
