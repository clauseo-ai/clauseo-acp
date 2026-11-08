//! Which CPU type a staged exe may have (ADR 0049, "Admin copy refresh", "The one move across
//! CPU types"). Its COFF `Machine` must be the running copy's ([`Trust::machine`]), with one
//! exception: on a natively ARM64 machine, an x64 copy takes a strictly newer ARM64 build, so
//! an x64 install there moves to the native build through the normal update. The move needs
//! all of these:
//!
//! 1. the running copy is x64, and so is the copy on disk the refresh replaces, read by handle
//!    ([`Refresher::admin_copy_machine`]) rather than assumed from the running process;
//! 2. the staged image is plain ARM64, neither ARM64EC nor ARM64X
//!    ([`pe_version::plain_machine`], which tells a hybrid by its CHPE metadata);
//! 3. the machine is natively ARM64 (`IsWow64Process2`, [`Refresher::native_machine`]);
//! 4. the signed version is strictly newer than the record's `AdminCopyVersion`: the equal
//!    version a same-CPU run may repair with never moves the copy.
//!
//! Anything else is [`RefreshExit::WrongMachine`] (42), the reverse move included: ARM64 to x64
//! would put a native copy back under emulation, a downgrade of the platform match. The checks
//! on the staged bytes come first, so only a staged plain ARM64 image on an x64 copy reads the
//! copy on disk or asks the machine.

use std::cmp::Ordering;

use super::pe_version::{self, MACHINE_ARM64, MACHINE_X64};
use super::{RefreshExit, Refresher, Refusal, Trust, version};

/// The staged image's CPU type, against `trust` and, for the one move, the copy on disk, the
/// machine and the versions: `recorded` is the record's `AdminCopyVersion`, `signed` the
/// staged exe's signed version.
pub(super) fn check(
    fs: &mut impl Refresher,
    staged: &[u8],
    trust: &Trust<'_>,
    recorded: &str,
    signed: &str,
) -> Result<(), Refusal> {
    let machine = pe_version::machine(staged);
    if machine == Some(trust.machine) {
        return Ok(());
    }
    cross_move(fs, staged, trust, recorded, signed).map_err(|why| {
        let found = machine.map_or_else(|| "none".to_owned(), |m| format!("{m:#06x}"));
        let line = format!(
            "the staged exe is for machine {found}, not this copy's {:#06x}, and {why}",
            trust.machine
        );
        (RefreshExit::WrongMachine, line)
    })
}

/// The x64-to-ARM64 move's conditions, in the module's order; `Err` says which one failed.
fn cross_move(
    fs: &mut impl Refresher,
    staged: &[u8],
    trust: &Trust<'_>,
    recorded: &str,
    signed: &str,
) -> Result<(), String> {
    if trust.machine != MACHINE_X64 {
        return Err("only an x64 copy moves to another CPU type".to_owned());
    }
    if pe_version::plain_machine(staged) != Some(MACHINE_ARM64) {
        return Err("an x64 copy moves only to a plain ARM64 build".to_owned());
    }
    let on_disk = fs
        .admin_copy_machine()
        .map_err(|why| format!("the copy in place has no readable CPU type ({why})"))?;
    if on_disk != Some(MACHINE_X64) {
        return Err("the copy in place isn't plain x64".to_owned());
    }
    let native = fs
        .native_machine()
        .map_err(|why| format!("the machine's CPU type is unknown ({why})"))?;
    if native != MACHINE_ARM64 {
        return Err("the machine isn't natively ARM64".to_owned());
    }
    if version::compare(recorded, signed) != Some(Ordering::Greater) {
        return Err(format!(
            "the move needs a version newer than the recorded {recorded}, not {signed}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
