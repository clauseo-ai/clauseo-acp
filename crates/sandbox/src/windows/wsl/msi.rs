//! WSL's pending MSI upgrade, installed as WSL's own installer installs it (ADR 0049, "WSL",
//! "Self-healing"), by the WSL upgrade task's SYSTEM run (`--wsl-upgrade`), before it re-applies
//! the deny. The mirror, at microsoft/WSL @19cd15ea:
//!
//! - **When**: the package's version newer than the MSI's `Version`, as `IsUpdateNeeded`
//!   compares them (`WslInstaller.cpp:142-165`) and as the launch check does
//!   (`upgrade::msi_behind`); a missing or empty `Version` isn't an upgrade (`upgrade.rs`).
//! - **Which MSI**: `wsl.msi` in the folder of the `WslInstaller` service's image
//!   (`GetMsiPackagePath`, `GetBasePath`, `WslInstaller.cpp:21-32`): the service's `ImagePath`,
//!   which only administrators can write, names a folder directly under
//!   `%ProgramFiles%\WindowsApps`, which only `TrustedInstaller` writes
//!   (`upgrade::installer_package`), and checked before use (`msi_verify.rs`): the package
//!   family's folder, no reparse point, the owner, Microsoft's signature, and the file held open
//!   until msiexec ends.
//! - **How**: `UpgradeViaMsi(path, L"SKIPMSIX=1")` (`WslInstaller.cpp:97-98`), which adds
//!   `REBOOT=ReallySuppress` and calls `MsiInstallProduct` with no UI (`install.cpp:385-406`,
//!   `MsiSetInternalUI(INSTALLUILEVEL_NONE…)` at `install.cpp:238`). Here: `msiexec.exe /i
//!   <path> /qn SKIPMSIX=1 REBOOT=ReallySuppress`, from System32, whose exit code is
//!   `MsiInstallProduct`'s. 0 and 3010 (`ERROR_SUCCESS_REBOOT_REQUIRED`) are success, as there
//!   (`WslInstaller.cpp:100-108`). WSL's verbose log isn't kept.
//!
//! A failed install is rolled back by Windows Installer, so WSL is left as it was. A wait past
//! [`MSI_WAIT`] stops waiting and leaves msiexec running: ending it mid-install could leave WSL
//! half-changed.
#![expect(
    unsafe_code,
    reason = "FFI to GetSystemDirectoryW and WaitForSingleObject"
)]

use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Command, Stdio};

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::core::{HSTRING, PCWSTR};

use super::WindowsWsl;
use super::msi_files::{HeldMsi, WindowsMsiFiles};
pub use super::msi_verify::MsiUpgrade;
use super::msi_verify::{InstallError, upgrade_with};
use super::upgrade::INSTALLER_SERVICE;
pub use super::upgrade::MSI_WAIT;
use crate::windows::record::read_units;

/// msiexec's arguments after the MSI's path: no UI, then `UpgradeViaMsi`'s properties.
pub const MSI_ARGS: [&str; 3] = ["/qn", "SKIPMSIX=1", "REBOOT=ReallySuppress"];
/// `ERROR_SUCCESS_REBOOT_REQUIRED`: installed, some files replaced at the next restart.
const REBOOT_REQUIRED: i32 = 3010;

/// Installs WSL's pending MSI upgrade, as the module says, once `msi_verify.rs`'s checks pass.
/// As LOCAL SYSTEM.
pub fn upgrade_pending(program_files: &Path) -> MsiUpgrade {
    let key = HSTRING::from(format!(
        r"SYSTEM\CurrentControlSet\Services\{INSTALLER_SERVICE}"
    ));
    let value = HSTRING::from("ImagePath");
    let kind = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let image = match read_units(PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), kind) {
        Ok(None) => return MsiUpgrade::NotPending,
        Ok(Some(units)) => {
            let text = units.split(|unit| *unit == 0).next().unwrap_or_default();
            String::from_utf16_lossy(text)
        }
        Err(error) => return MsiUpgrade::Failed(format!("the installer service's image: {error}")),
    };
    let apps = program_files.join("WindowsApps");
    let reader = WindowsWsl::reader();
    upgrade_with(
        &WindowsMsiFiles,
        &reader,
        &image,
        &apps.to_string_lossy(),
        install,
    )
}

/// Runs msiexec on `msi` with [`MSI_ARGS`] and waits up to [`MSI_WAIT`].
#[expect(
    clippy::disallowed_methods,
    reason = "msiexec, a Windows tool the SYSTEM task runs to install WSL's MSI, not an agent"
)]
fn install(msi: &str, _held: &HeldMsi) -> Result<(), InstallError> {
    let msiexec = Path::new(&system_folder().map_err(InstallError::Failed)?).join("msiexec.exe");
    let mut child = Command::new(msiexec)
        .arg("/i")
        .arg(msi)
        .args(MSI_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| InstallError::Failed(format!("msiexec couldn't start: {error}")))?;
    let millis = u32::try_from(MSI_WAIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the child's process handle lives as long as `child`, past the call.
    let waited = unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), millis) };
    if waited != WAIT_OBJECT_0 {
        let secs = MSI_WAIT.as_secs();
        let why = format!("msiexec didn't end within {secs} s; it was left running");
        return Err(InstallError::StillInstalling(why));
    }
    let status = child
        .try_wait()
        .map_err(|error| InstallError::Failed(format!("msiexec's exit: {error}")))?
        .ok_or_else(|| {
            InstallError::Failed("msiexec's exit wasn't there after its end".to_owned())
        })?;
    match status.code() {
        Some(0 | REBOOT_REQUIRED) => Ok(()),
        Some(code) => Err(InstallError::Failed(format!("msiexec ended with {code}"))),
        None => Err(InstallError::Failed(
            "msiexec ended with no code".to_owned(),
        )),
    }
}

/// System32, from Windows, never from the environment.
fn system_folder() -> Result<String, String> {
    let mut buffer = [0u16; 260];
    // SAFETY: `buffer` is writable for its length.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) };
    let len = usize::try_from(len).map_err(|error| error.to_string())?;
    let units = buffer
        .get(..len)
        .filter(|units| !units.is_empty() && len < 260);
    let units = units.ok_or_else(|| "GetSystemDirectoryW failed".to_owned())?;
    String::from_utf16(units).map_err(|error| error.to_string())
}
