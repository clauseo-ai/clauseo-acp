//! The move from x64 to arm64 on Windows on ARM (stage F's tier 1 design, "App thread"): an x64
//! per-user install running under ARM64's emulation asks the release for its arm64 installer
//! (`windows-aarch64`) at its next update, and updates x64 in place when the release has none.
//!
//! - [`emulated_on_arm64`]: this is the x64 build and the machine is ARM64 (`IsWow64Process2`'s
//!   native machine, as `clauseo_sandbox`'s `windows/detect.rs` reads it). Any error is false.
//! - [`move_to_arm64`]: the gate, a plain function over what was read. It also needs a per-user
//!   install (per machine has no updater; IT deploys the arm64 machine installer) and an admin
//!   copy that is not set up or up to date. An up-to-date copy is this build, whose refresh
//!   takes an arm64 set-up program (stage C); one whose refresh lags would refuse it (code 42)
//!   and stay at the old version for good.
//! - [`Target`]: which installer a check asked for. The check that found the update records it
//!   with the update (`plugin::Pending`), and the download's and the install's file binding and
//!   the install's `restore_update` all take it from there, never from a new detection.

use clauseo_host::InstallKind;

use crate::admin_refresh::RefreshDecision;

/// The release's key for the arm64 per-user installer (`latest-json.mjs`), which a custom
/// target looks up alone: the plugin tries no other key for it.
pub(super) const ARM64_KEY: &str = "windows-aarch64";

/// Which installer an update is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// This build's own: the plugin's default keys, the arch it was compiled for.
    Native,
    /// The arm64 per-user installer, for an x64 build emulated on ARM64.
    Arm64,
}

impl Target {
    /// The plugin's custom target for this choice; none for this build's own.
    pub(super) fn plugin_target(self) -> Option<&'static str> {
        match self {
            Target::Native => None,
            Target::Arm64 => Some(ARM64_KEY),
        }
    }

    /// The OS and Rust `target_arch` the update's file is built for, which name it
    /// (`file_binding::update_file_name`): this build's own, or Windows on arm64, the one
    /// platform [`ARM64_KEY`] names.
    pub(super) fn platform(self) -> (&'static str, &'static str) {
        match self {
            Target::Native => (std::env::consts::OS, std::env::consts::ARCH),
            Target::Arm64 => ("windows", "aarch64"),
        }
    }
}

/// The admin copy's state as the gate reads it: the start-up compare's decision
/// (`admin_refresh::refresh_needed`), or none when the record or the installed set-up program
/// could not be read.
pub(super) type AdminCopy = Option<RefreshDecision>;

/// Whether an update moves this app to the arm64 build: only when it is the x64 build emulated
/// on ARM64, installed per user, with an admin copy that is not set up or up to date. Anything
/// else keeps updating this build in place.
pub(super) fn move_to_arm64(emulated: bool, kind: InstallKind, admin: AdminCopy) -> bool {
    emulated
        && kind == InstallKind::CurrentUser
        && matches!(
            admin,
            Some(RefreshDecision::NotSetUp | RefreshDecision::UpToDate)
        )
}

/// The gate for this machine, read now: [`emulated_on_arm64`], `kind`, and the admin copy, read
/// only when the first two hold. It reads the registry and hashes a file, so it runs off the
/// async workers. The plugin adapter's default probe.
pub(super) fn probe(kind: InstallKind) -> bool {
    let emulated = emulated_on_arm64();
    // Off Windows there is no record, so no admin copy; never read there, since nothing is
    // emulated.
    #[cfg(not(windows))]
    let admin_copy = || Some(crate::admin_refresh::refresh_needed(None, None));
    let admin = if emulated && kind == InstallKind::CurrentUser {
        admin_copy()
    } else {
        None
    };
    move_to_arm64(emulated, kind, admin)
}

/// Whether this is the x64 build running under ARM64's emulation. Always false off Windows, and
/// in any build but x64's.
pub(crate) fn emulated_on_arm64() -> bool {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        native_is_arm64()
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        false
    }
}

/// At start, for a per-machine install that is emulated: one host.log line. It has no updater,
/// so IT moves it, with the arm64 machine installer over this one. Built where its caller
/// (`choice::updater`) is: Linux has no updater.
#[cfg(not(target_os = "linux"))]
pub(super) fn log_per_machine(kind: InstallKind, emulated: bool) {
    if kind == InstallKind::PerMachine && emulated {
        tracing::info!(
            target: super::install::LIFECYCLE,
            "emulated x64 on ARM64; IT can deploy the arm64 machine installer"
        );
    }
}

/// `IsWow64Process2`'s native machine is ARM64. An error is false: the move never happens on a
/// guess.
#[cfg(all(windows, target_arch = "x86_64"))]
#[expect(unsafe_code, reason = "FFI to IsWow64Process2")]
fn native_is_arm64() -> bool {
    use windows::Win32::System::SystemInformation::{IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_ARM64};
    use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    let mut process = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; both out-pointers
    // are live for the call.
    let read =
        unsafe { IsWow64Process2(GetCurrentProcess(), &raw mut process, Some(&raw mut native)) };
    match read {
        Ok(()) => native == IMAGE_FILE_MACHINE_ARM64,
        Err(error) => {
            tracing::debug!(%error, "the machine's architecture was not read: x64 in place");
            false
        }
    }
}

/// The admin copy's state, read as the start-up compare reads it: the record's hash and the
/// installed set-up program's. A failed read is none.
#[cfg(windows)]
fn admin_copy() -> AdminCopy {
    use clauseo_sandbox::windows::refresh::{installed_setup_sha256, recorded_admin_copy_sha256};
    let recorded = recorded_admin_copy_sha256().ok()?;
    let installed = match recorded {
        None => None,
        Some(_) => installed_setup_sha256().ok()?,
    };
    Some(crate::admin_refresh::refresh_needed(
        recorded.as_deref(),
        installed.as_deref(),
    ))
}
