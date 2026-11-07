//! A launch's network permit (S15 PR7): a dynamic WFP session, opened as the desktop user, in
//! which the launch adds its own permit filters after checking set-up's block filters
//! ([`check_setup`]). The session closes when the permit drops, and WFP deletes its filters
//! with it, on a crash too; so an agent left running after its host is gone has no permit.

use super::check::{NetworkGap, check_setup};
use super::engine::Engine;
use super::spec::{Namespace, checked_ports, permit_filters, permit_ports};
use crate::windows::sids::Sid;
use crate::{Error, Policy};

/// One launch's permit: while it lives, the pool user `user` may reach `127.0.0.1` over TCP on
/// exactly [`LaunchPermit::ports`], and nothing else; dropping it (or [`LaunchPermit::close`])
/// takes that away.
#[must_use = "dropping the permit takes the agent's network away at once"]
#[derive(Debug)]
pub struct LaunchPermit {
    engine: Engine,
    user: Sid,
    ports: Vec<u16>,
}

impl LaunchPermit {
    /// The permit for a tier-B launch of `policy` as the pool user `user` (the matter's, from
    /// `matters.json`): `policy`'s proxy ports and [`Loopback::Ports`](crate::Loopback::Ports).
    ///
    /// Fails closed with [`Error::NetworkUnfiltered`]: set-up's objects missing, changed,
    /// unreadable or not naming `user` (`sandbox.setup_required`); `Loopback::Any`, a port
    /// outside 61080-61179, or a permit WFP refused (`agent.launch_failed`). The launch must
    /// not start the agent then.
    pub fn open(user: &Sid, policy: &Policy) -> Result<LaunchPermit, Error> {
        let ports = permit_ports(policy).map_err(unfiltered)?;
        LaunchPermit::open_ports(&Namespace::production(), user, &ports)
    }

    /// [`LaunchPermit::open`] for `ports` in `ns`: for the VM harness and the elevated tests.
    pub fn open_ports(ns: &Namespace, user: &Sid, ports: &[u16]) -> Result<LaunchPermit, Error> {
        let ports = checked_ports(ports).map_err(unfiltered)?;
        let permit_failed = |error: super::check::WfpError| NetworkGap::PermitFailed {
            call: error.call,
            code: error.code,
        };
        let mut engine = Engine::open(true, &ns.display)
            .map_err(permit_failed)
            .map_err(unfiltered)?;
        check_setup(ns, user, &mut engine).map_err(unfiltered)?;
        for spec in permit_filters(ns, user, &ports) {
            engine
                .add_filter(ns, &spec)
                .map_err(permit_failed)
                .map_err(unfiltered)?;
        }
        Ok(LaunchPermit {
            engine,
            user: user.clone(),
            ports,
        })
    }

    /// The pool user the permit is for.
    pub fn user(&self) -> &Sid {
        &self.user
    }

    /// The loopback ports it opens, sorted.
    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    /// Closes the permit's session; WFP deletes its filters. The same as dropping it, said
    /// where the session ends.
    pub fn close(self) {
        let LaunchPermit { engine, .. } = self;
        drop(engine);
    }
}

/// `cause` as the crate's error.
fn unfiltered(cause: NetworkGap) -> Error {
    Error::NetworkUnfiltered { cause }
}
