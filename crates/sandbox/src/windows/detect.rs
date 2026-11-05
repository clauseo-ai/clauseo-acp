//! What this Windows can give, read once per process by `detect`: the machine's architecture and
//! the build number. Tier B needs x64 and Windows 10 22H2 or later.
#![expect(unsafe_code, reason = "FFI to IsWow64Process2")]

use windows::Win32::System::SystemInformation::{IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_ARM64};
use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
use windows::core::w;

use super::record::read_string;
use crate::detect::OsFacts;

/// The machine's architecture and build. A failed architecture read leaves the build as the
/// error, so detection reports a failed probe and never guesses.
pub(crate) fn os_facts() -> OsFacts {
    let build = build();
    match native_machine() {
        Ok(machine) => OsFacts {
            arm64: machine == IMAGE_FILE_MACHINE_ARM64,
            build,
        },
        Err(error) => OsFacts {
            arm64: false,
            build: Err(format!("the machine's architecture: {error}")),
        },
    }
}

/// The machine's own architecture from `IsWow64Process2`, so an x64 build running under ARM64's
/// emulation sees ARM64, not the x64 it was built for.
fn native_machine() -> windows::core::Result<IMAGE_FILE_MACHINE> {
    let mut process = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; both out-pointers
    // are live for the call.
    unsafe { IsWow64Process2(GetCurrentProcess(), &raw mut process, Some(&raw mut native)) }?;
    Ok(native)
}

/// `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\CurrentBuildNumber`, a `REG_SZ` every
/// Windows has. `GetVersionExW` would lie to an exe without a compatibility manifest.
fn build() -> Result<u32, String> {
    let text = read_string(
        w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
        w!("CurrentBuildNumber"),
    )
    .map_err(|error| format!("CurrentBuildNumber: {error}"))?
    .ok_or_else(|| "CurrentBuildNumber: missing".to_owned())?;
    text.trim()
        .parse()
        .map_err(|_not_a_number| format!("CurrentBuildNumber: {text:?} is not a number"))
}
