//! A tier-B launch's network check (S15 PR7): set-up's filters are read as the desktop user
//! (set-up's read-only grants) and checked ([`check_setup`]) before the agent starts. Nothing is
//! added: the static permits set-up wrote open 61080-61179 to each pool user, and the
//! listeners' per-launch secrets keep matters apart there.

use super::check::{NetworkGap, check_setup};
use super::engine::Engine;
use super::spec::{Namespace, checked_ports, permit_ports};
use crate::windows::sids::Sid;
use crate::{Error, Policy};

/// Checks that a tier-B launch of `policy` as the pool user `user` (the matter's, from
/// `matters.json`) has its network filtered, before the agent starts.
///
/// Fails closed with [`Error::NetworkUnfiltered`]: set-up's objects missing, changed,
/// unreadable, joined by a filter set-up didn't write, or not covering `user`
/// (`sandbox.setup_required`); `Loopback::Any`, a port outside 61080-61179, or WFP not
/// answering (`agent.launch_failed`). The launch must not start the agent then.
pub fn check_launch(user: &Sid, policy: &Policy) -> Result<(), Error> {
    let ports = permit_ports(policy).map_err(unfiltered)?;
    check_launch_in(&Namespace::production(), user, &ports)
}

/// [`check_launch`] for `ports` in `ns`: for the VM harness and the elevated tests.
pub fn check_launch_in(ns: &Namespace, user: &Sid, ports: &[u16]) -> Result<(), Error> {
    checked_ports(ports).map_err(unfiltered)?;
    let mut engine = Engine::open(false, &ns.display)
        .map_err(|error| NetworkGap::EngineUnavailable {
            call: error.call,
            code: error.code,
        })
        .map_err(unfiltered)?;
    check_setup(ns, user, &mut engine).map_err(unfiltered)
}

/// `cause` as the crate's error.
fn unfiltered(cause: NetworkGap) -> Error {
    Error::NetworkUnfiltered { cause }
}
