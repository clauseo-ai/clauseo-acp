//! The seams: [`WslRead`], what the launch check reads as the lawyer, and [`WslWrite`], what
//! the set-up, the refresh and the removal change, elevated. `os.rs` is the real registry; the
//! tests fake both (`fake.rs`).

use super::record::{AppId, WslRecord};
use crate::windows::sids::Sid;

/// A registry call that failed: what failed, in words that name no path or SID (the refresh
/// puts them in the Event Log).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct WslError(pub String);

/// A COM class's registration (`HKLM\SOFTWARE\Classes\CLSID\{...}`, 64-bit view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassEntry {
    /// Not registered.
    Missing,
    /// Registered with no `AppID` value: COM would launch it under the default permission.
    NoAppId,
    /// Registered under this `AppID`.
    AppId(AppId),
}

/// An `AppID`'s `LaunchPermission` as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// The `AppID`'s key isn't there.
    NoKey,
    /// The key is there without the value: COM uses `DefaultLaunchPermission`.
    Absent,
    /// The value's bytes.
    Bytes(Vec<u8>),
}

/// What the launch check reads, as the lawyer, unelevated. Every key it reads is readable by
/// `Users` (the VM's R5c read).
pub trait WslRead {
    /// Whether the service `name` is installed (its key under `Services`).
    fn service_exists(&self, name: &str) -> Result<bool, WslError>;
    /// The class `clsid`'s registration.
    fn class(&self, clsid: &str) -> Result<ClassEntry, WslError>;
    /// Every `AppID` whose `LocalService` is one of `services`, letter case ignored.
    fn service_app_ids(&self, services: &[&str]) -> Result<Vec<AppId>, WslError>;
    /// `app_id`'s `LaunchPermission`.
    fn launch_permission(&self, app_id: &AppId) -> Result<Permission, WslError>;
    /// Whether `HKLM\SOFTWARE\Classes\PackagedCom\ClassIndex` holds any of [`CLASSES`]'
    /// CLSIDs: a packaged registration, whose launch permission is the package's and overrides
    /// the `AppID`'s (the pre-2.0 Store WSL's `LxssUserSession`).
    ///
    /// [`CLASSES`]: super::CLASSES
    fn packaged_class(&self) -> Result<bool, WslError>;
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Lxss\MSI`'s `Version` (`REG_SZ`), the
    /// installed MSI's version as WSL records it; `None` when it or the key is missing.
    fn msi_version(&self) -> Result<Option<String>, WslError>;
    /// The full names under `HKLM\SOFTWARE\Classes\PackagedCom\Package` that start with
    /// [`PACKAGE_NAME`] and `_`: the WSL package's packaged-COM registrations.
    ///
    /// [`PACKAGE_NAME`]: super::PACKAGE_NAME
    fn wsl_packages(&self) -> Result<Vec<String>, WslError>;
    /// The pool group's SID, as the machine's record names it (`GroupSid`); `None` when it is
    /// missing or isn't a SID.
    fn group(&self) -> Option<Sid>;
}

/// What the elevated runs change, on top of what they read.
pub trait WslWrite: WslRead {
    /// `HKLM\SOFTWARE\Microsoft\Ole\DefaultLaunchPermission`, when there is one.
    fn default_launch_permission(&self) -> Result<Option<Vec<u8>>, WslError>;
    /// Sets `app_id`'s `LaunchPermission` (`REG_BINARY`) to `bytes`, through a backup-restore
    /// open with `SeRestorePrivilege`, so the key's owner and DACL stay as they are. A key that
    /// isn't there is an error, and is never made.
    fn write_launch_permission(&mut self, app_id: &AppId, bytes: &[u8]) -> Result<(), WslError>;
    /// Deletes `app_id`'s `LaunchPermission`, opened as [`WslWrite::write_launch_permission`]
    /// opens it; one already gone is `Ok`.
    fn delete_launch_permission(&mut self, app_id: &AppId) -> Result<(), WslError>;
    /// The targets the machine record holds; none when its values are missing.
    fn recorded(&self) -> Result<WslRecord, WslError>;
    /// Writes `record` to the machine record, which must be there (never made).
    fn record(&mut self, record: &WslRecord) -> Result<(), WslError>;
    /// Deletes the two values from the machine record; gone already is `Ok`.
    fn forget(&mut self) -> Result<(), WslError>;
}
