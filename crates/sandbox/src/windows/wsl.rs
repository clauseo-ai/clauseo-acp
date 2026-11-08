//! WSL denied to the pool (ADR 0049, "WSL"). A pool user who can start WSL gets a Linux that
//! WFP doesn't filter (WSL2's VM) and a service that acts for it as SYSTEM, so tier B adds a
//! deny for `ClauseoSandboxUsers` to the COM launch permission of WSL's classes: RPCSS checks it
//! at every activation, against a running server too (the VM's R5c rows T1, T2), and every
//! route into WSL (`wsl.exe`, `bash.exe`, `wslapi.dll`, `\\wsl$`, a COM call from PowerShell)
//! goes through an activation.
//!
//! - **Targets** ([`resolve`]), found at run time, never hard-coded: every `AppID` whose
//!   `LocalService` is `WSLService` or `LxssManager`, and the `AppID` of each of [`CLASSES`]
//!   that is registered. The value is `HKLM\SOFTWARE\Classes\AppID\{...}\LaunchPermission`, a
//!   self-relative security descriptor, read and written in the 64-bit view.
//! - **The edit** ([`LaunchSd`], `sd.rs`, pure): `(D;;CCDCLCSWRP;;;G)` ([`DENY_MASK`]) at DACL
//!   index 0, every other part and entry kept; nothing to do when a deny for G with at least
//!   [`NEEDED_MASK`] already comes before every allow. A missing value is built from
//!   `DefaultLaunchPermission`, else from COM's built-in default ([`LaunchSd::built_in_default`]),
//!   and recorded as made by us.
//! - **Apply** ([`apply`], elevated): the set-up after the group exists, and the SYSTEM task
//!   `\Clauseo\SandboxRefresh` on every run (at logon, and on Windows Installer's events in the
//!   Application log, which a WSL update writes). Each new value is checked before it is
//!   written, written with `SeRestorePrivilege` through a backup-restore open (the key's owner
//!   and DACL are never touched), then read back and checked again. The targets are recorded
//!   first ([`WslRecord`], `WslDenyAppIds` and `WslDenyAbsent` in the machine record).
//! - **The launch check** ([`check`], unelevated, as the lawyer): with WSL present, a target
//!   whose deny is missing or misplaced refuses the launch with `Blocked::WslDenyMissing`; with
//!   `WSLService` present, one that can't be read or parsed, a class with no `AppID`, or a
//!   packaged-COM registration of WSL's class (which would bypass the `AppID`) refuses with
//!   `Blocked::WslDenyUnverifiable`. With no WSL at all nothing is refused.
//! - **Removal** ([`remove`], `--remove`): only our denies come off each recorded target, and a
//!   value we made goes once what is left is the default it was made from. No old value is ever
//!   written back: it may be older than a WSL update since.
//!
//! The descriptor edits, the targets' rules and the order of the steps hold no Windows call:
//! they build and are tested on every OS over [`WslRead`] and [`WslWrite`]; `os.rs` is the real
//! registry, which `setup/wsl_tests.rs` (elevated) drives on a throwaway `AppID`.

mod apply;
mod check;
#[cfg(windows)]
mod os;
mod record;
mod sd;
mod seam;

pub use apply::{WslApplied, WslRemoved, apply, remove};
pub use check::{Found, Resolved, check, resolve};
#[cfg(windows)]
pub use os::WindowsWsl;
pub use record::{AppId, RECORD_ABSENT, RECORD_APP_IDS, WslRecord};
pub use sd::{DENY_MASK, LaunchSd, NEEDED_MASK, SdError};
pub use seam::{ClassEntry, Permission, WslError, WslRead, WslWrite};

/// The lifted WSL's service (the MSI's `wslservice.exe`), which runs WSL2's VMs.
pub const WSL_SERVICE: &str = "WSLService";
/// The inbox WSL's service, from the OS image.
pub const LXSS_MANAGER: &str = "LxssManager";
/// The services whose `AppID`s are targets.
pub const SERVICES: [&str; 2] = [WSL_SERVICE, LXSS_MANAGER];
/// `LxssUserSession`: every `wsl.exe` operation, WSL1 and WSL2.
pub const LXSS_USER_SESSION: &str = "{A9B7A1B9-0671-405C-95F1-E0612CB4CE7E}";
/// `WSLCSessionManager`: WSL's containers, which make their own VM.
pub const WSLC_SESSION_MANAGER: &str = "{A9B7A1B9-0671-405C-95F1-E0612CB4CE8F}";
/// `LxssUserSessionInBox`: the inbox service's class.
pub const LXSS_USER_SESSION_INBOX: &str = "{4F476546-B412-4579-B64C-123DF331E3D6}";
/// The classes whose `AppID`s are targets when they are registered, whatever their `AppID`'s
/// `LocalService` says.
pub const CLASSES: [&str; 3] = [
    LXSS_USER_SESSION,
    WSLC_SESSION_MANAGER,
    LXSS_USER_SESSION_INBOX,
];

#[cfg(test)]
mod fake;
#[cfg(test)]
pub(crate) use fake::{FakeWsl, NoWsl};
#[cfg(test)]
mod tests;
