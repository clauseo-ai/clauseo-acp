//! WSL's pending MSI upgrade, installed as WSL's own installer installs it (ADR 0049, "WSL",
//! "Self-healing"), by the WSL deny task's SYSTEM run (`--wsl-deny`), before it re-applies the
//! deny. The mirror, at microsoft/WSL @19cd15ea:
//!
//! - **When**: the package's version newer than the MSI's `Version`, as `IsUpdateNeeded`
//!   compares them (`WslInstaller.cpp:142-165`) and as the launch check does
//!   ([`msi_behind`]); a missing or empty `Version` isn't an upgrade (`upgrade.rs`).
//! - **Which MSI**: `wsl.msi` in the folder of the `WslInstaller` service's image
//!   (`GetMsiPackagePath`, `GetBasePath`, `WslInstaller.cpp:21-32`): the service's `ImagePath`,
//!   which only administrators can write, names a folder directly under
//!   `%ProgramFiles%\WindowsApps`, which only `TrustedInstaller` writes ([`installer_package`]).
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
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::core::{HSTRING, PCWSTR};

use super::WindowsWsl;
use super::seam::WslRead;
use super::upgrade::{INSTALLER_SERVICE, PACKAGE_MSI, Version, installer_package, msi_behind};
use crate::windows::record::read_units;

/// How long the run waits for msiexec: an upgrade takes tens of seconds; the task's limit is 5
/// minutes (`rules::WSL_DENY_EXECUTION_LIMIT`), and the deny is re-applied after the wait.
pub const MSI_WAIT: Duration = Duration::from_secs(240);
/// msiexec's arguments after the MSI's path: no UI, then `UpgradeViaMsi`'s properties.
pub const MSI_ARGS: [&str; 3] = ["/qn", "SKIPMSIX=1", "REBOOT=ReallySuppress"];
/// `ERROR_SUCCESS_REBOOT_REQUIRED`: installed, some files replaced at the next restart.
const REBOOT_REQUIRED: i32 = 3010;

/// What [`upgrade_pending`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsiUpgrade {
    /// No upgrade was pending, or there is no `WslInstaller` service to take the MSI from.
    NotPending,
    /// The MSI at `from` was upgraded to the package's version `to`.
    Upgraded {
        /// The MSI's version before.
        from: String,
        /// The package's version.
        to: Version,
    },
    /// The upgrade was owed and failed, or couldn't be tried: why, with codes, never a path.
    Failed(String),
}

/// Installs WSL's pending MSI upgrade, as the module says. As LOCAL SYSTEM.
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
    let Some((folder, package)) = installer_package(&image, &apps.to_string_lossy()) else {
        let why = "the installer service's image isn't in a WSL package's folder";
        return MsiUpgrade::Failed(why.to_owned());
    };
    let msi = match WindowsWsl::reader().msi_version() {
        Ok(msi) => msi,
        Err(error) => return MsiUpgrade::Failed(format!("the MSI's version: {error}")),
    };
    if !msi_behind(package, msi.as_deref()) {
        return MsiUpgrade::NotPending;
    }
    let path = Path::new(&folder).join(PACKAGE_MSI);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return MsiUpgrade::Failed(format!("{PACKAGE_MSI} isn't a file")),
        Err(error) => return MsiUpgrade::Failed(format!("{PACKAGE_MSI}: {error}")),
    }
    let from = msi.unwrap_or_default();
    match install(&path) {
        Ok(()) => MsiUpgrade::Upgraded { from, to: package },
        Err(why) => MsiUpgrade::Failed(why),
    }
}

/// Runs msiexec on `msi` with [`MSI_ARGS`] and waits up to [`MSI_WAIT`].
#[expect(
    clippy::disallowed_methods,
    reason = "msiexec, a Windows tool the SYSTEM task runs to install WSL's MSI, not an agent"
)]
fn install(msi: &Path) -> Result<(), String> {
    let msiexec = Path::new(&system_folder()?).join("msiexec.exe");
    let mut child = Command::new(msiexec)
        .arg("/i")
        .arg(msi)
        .args(MSI_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("msiexec couldn't start: {error}"))?;
    let millis = u32::try_from(MSI_WAIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the child's process handle lives as long as `child`, past the call.
    let waited = unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), millis) };
    if waited != WAIT_OBJECT_0 {
        let secs = MSI_WAIT.as_secs();
        return Err(format!(
            "msiexec didn't end within {secs} s; it was left running"
        ));
    }
    let status = child
        .try_wait()
        .map_err(|error| format!("msiexec's exit: {error}"))?
        .ok_or_else(|| "msiexec's exit wasn't there after its end".to_owned())?;
    match status.code() {
        Some(0 | REBOOT_REQUIRED) => Ok(()),
        Some(code) => Err(format!("msiexec ended with {code}")),
        None => Err("msiexec ended with no code".to_owned()),
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
