//! A tier-B launch's network check (S15 PR7): set-up's filters are read as the desktop user
//! (set-up's read-only grants) and checked ([`check_setup`]) before the agent starts. Nothing is
//! added: set-up's per-slot filters open each pool user's own slot's ports to it alone (0049,
//! "Loopback ports per slot"), and the check gives the launch that slot, whose ports its
//! listeners and the browser's claims then use (`ports`).

use super::check::{NetworkGap, check_setup};
use super::engine::Engine;
use super::os_ports::check_ports;
use super::spec::{Namespace, checked_ports, permit_ports};
use crate::ports::Slot;
use crate::windows::sids::Sid;
use crate::{Error, Policy};

/// Checks that a tier-B launch of `policy` as the pool user `user` (the matter's, from
/// `matters.json`) has its network filtered, before the agent starts; `user`'s slot, whose
/// ports the launch's listeners ([`ListenerFor::TierB`](crate::ports::ListenerFor)) and
/// browser claims use. A launch that binds its listeners first calls [`check_slot`] for the
/// slot, then this with the policy that names their ports.
///
/// Fails closed with [`Error::NetworkUnfiltered`]: set-up's objects missing, changed,
/// unreadable, joined by a filter set-up didn't write, an earlier set-up's, or not covering
/// `user` (`sandbox.setup_required`); `Loopback::Any`, a port outside `user`'s slot, or WFP
/// not answering (`agent.launch_failed`). The launch must not start the agent then.
pub fn check_launch(user: &Sid, policy: &Policy) -> Result<Slot, Error> {
    let ports = permit_ports(policy).map_err(unfiltered)?;
    check_launch_in(&Namespace::production(), user, &ports)
}

/// `user`'s slot as set-up's filters give it, checked as [`check_launch`] checks them, before
/// any port is bound.
pub fn check_slot(user: &Sid) -> Result<Slot, Error> {
    check_launch_in(&Namespace::production(), user, &[])
}

/// [`check_launch`] for `ports` in `ns`: for the VM harness and the elevated tests.
pub fn check_launch_in(ns: &Namespace, user: &Sid, ports: &[u16]) -> Result<Slot, Error> {
    let mut engine = Engine::open(false, &ns.display)
        .map_err(|error| NetworkGap::EngineUnavailable {
            call: error.call,
            code: error.code,
        })
        .map_err(unfiltered)?;
    let slot = check_setup(ns, user, &mut engine).map_err(unfiltered)?;
    checked_ports(slot, ports).map_err(unfiltered)?;
    check_ports(true).map_err(unfiltered)?;
    Ok(slot)
}

/// `cause` as the crate's error.
fn unfiltered(cause: NetworkGap) -> Error {
    Error::NetworkUnfiltered { cause }
}
