//! The order the outer runner makes the agent's window station and desktop in, apart from its
//! calls so the tests hold it on every OS (0049, "A window station and desktop of the agent's
//! own"). The outer runs as the desktop user, unelevated, and such a user can't make a named
//! window station (`\Sessions\<n>\Windows\WindowStations` grants its `CreateChild` to
//! `Administrators` alone: error 5). So:
//!
//! 1. the pool user is logged on (`LOGON32_LOGON_INTERACTIVE`, with the password the outer holds
//!    for the inner runner's logon; a batch logon fails with 1385);
//! 2. the outer acts as that logon while it makes an unnamed station, which Windows names
//!    `Service-0x0-<logon LUID>$`, one per logon, with [`station_creation_sddl`]; then it goes
//!    back to itself on every path ([`Reverting`]), and the logon's token is closed;
//! 3. the outer opens the station by its name as itself and makes itself its owner;
//! 4. it makes the desktop `Default` on it ([`desktop_creation_sddl`]);
//! 5. it sets both last descriptors ([`station_sddl`], [`desktop_sddl`]): protected, labelled
//!    Low.
//!
//! Every failure refuses the launch; there is no other station to fall back to (never
//! `WinSta0`). The station goes with its last handle: the outer's, the inner runner's and the
//! agent's processes' own.

use crate::windows::sids::Sid;
use crate::windows::token::{
    desktop_creation_sddl, desktop_sddl, station_creation_sddl, station_sddl,
};

/// The calls; Windows' are `desktop.rs`'s.
pub(crate) trait StationSteps {
    /// The pool user's logon token.
    type Logon;
    /// A window station handle.
    type Station;
    /// A desktop handle.
    type Desktop;
    /// A failure, naming its call; it refuses the launch.
    type Error;
    /// Logs the pool user on, interactive.
    fn log_on(&mut self) -> Result<Self::Logon, Self::Error>;
    /// Makes this thread act as `logon`'s user.
    fn impersonate(&mut self, logon: &Self::Logon) -> Result<(), Self::Error>;
    /// Makes an unnamed window station with the descriptor `sddl`.
    fn create_station(&mut self, sddl: &str) -> Result<Self::Station, Self::Error>;
    /// Makes this thread itself again. It can't fail: Windows' ends the process when Windows
    /// refuses, since nothing is safe to run on a thread still acting as the pool user.
    fn revert(&mut self);
    /// Closes the logon's token; the station outlives it.
    fn close_logon(&mut self, logon: Self::Logon);
    /// The station's name.
    fn name(&mut self, station: &Self::Station) -> Result<String, Self::Error>;
    /// Opens the station `name` as this process's own user.
    fn open_station(&mut self, name: &str) -> Result<Self::Station, Self::Error>;
    /// Makes `owner` the station's owner.
    fn set_owner(&mut self, station: &Self::Station, owner: &Sid) -> Result<(), Self::Error>;
    /// Makes the desktop `Default` on `station` with the descriptor `sddl`.
    fn create_desktop(
        &mut self,
        station: &Self::Station,
        sddl: &str,
    ) -> Result<Self::Desktop, Self::Error>;
    /// Sets the station's protected DACL and label from `sddl`.
    fn secure_station(&mut self, station: &Self::Station, sddl: &str) -> Result<(), Self::Error>;
    /// Sets the desktop's protected DACL and label from `sddl`.
    fn secure_desktop(&mut self, desktop: &Self::Desktop, sddl: &str) -> Result<(), Self::Error>;
}

/// Who the descriptors name.
pub(crate) struct StationSids<'a> {
    /// The desktop user: the outer runner's own user, the station's last owner.
    pub(crate) creator: &'a Sid,
    /// The run's pool user.
    pub(crate) pool_user: &'a Sid,
    /// The matter's SID, when the launch restricts with one.
    pub(crate) matter: Option<&'a Sid>,
}

/// The station and desktop made, both set up, with the station's name.
pub(crate) struct Made<S, D> {
    pub(crate) station: S,
    pub(crate) desktop: D,
    pub(crate) name: String,
}

/// Runs the steps in order; the station and desktop, or the first failure.
pub(crate) fn make_station<S: StationSteps>(
    steps: &mut S,
    sids: &StationSids<'_>,
) -> Result<Made<S::Station, S::Desktop>, S::Error> {
    let logon = steps.log_on()?;
    let created = create_as(steps, &logon, sids.creator);
    steps.close_logon(logon);
    let created = created?;
    let name = steps.name(&created)?;
    let station = steps.open_station(&name)?;
    // The outer's own handle holds the station from here.
    drop(created);
    steps.set_owner(&station, sids.creator)?;
    let desktop = steps.create_desktop(&station, &desktop_creation_sddl(sids.creator))?;
    steps.secure_station(
        &station,
        &station_sddl(sids.creator, sids.pool_user, sids.matter),
    )?;
    steps.secure_desktop(
        &desktop,
        &desktop_sddl(sids.creator, sids.pool_user, sids.matter),
    )?;
    Ok(Made {
        station,
        desktop,
        name,
    })
}

/// The station made while this thread acts as `logon`'s user; the thread is itself again when
/// this returns, whatever happened ([`Reverting`]'s drop, a failed impersonation included).
fn create_as<S: StationSteps>(
    steps: &mut S,
    logon: &S::Logon,
    creator: &Sid,
) -> Result<S::Station, S::Error> {
    let reverting = Reverting(steps);
    reverting.0.impersonate(logon)?;
    reverting.0.create_station(&station_creation_sddl(creator))
}

/// Reverts on drop: every path out of [`create_as`], an unwind too.
struct Reverting<'a, S: StationSteps>(&'a mut S);

impl<S: StationSteps> Drop for Reverting<'_, S> {
    fn drop(&mut self) {
        self.0.revert();
    }
}

#[cfg(test)]
mod tests;
