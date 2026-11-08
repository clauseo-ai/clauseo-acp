//! A launch's reservation (PR6): the matter's account and network slot, held for one session
//! from `reserve` to its launch. On every OS, so the host's seam names one type: Windows tier B
//! hands out a pooled one (`windows::Launchers::reserve`); macOS's adapter makes one with no
//! slot and no account ([`Reservation::unpooled`]); the host's fake launchers make a fixed one
//! ([`Reservation::fixed`], `test-support`).

use std::sync::Arc;

use crate::windows::pool::PoolUser;

/// The matter's account and slot, held for one session, from `reserve` through `launch`.
/// Dropped before its launch, its claim releases them and undoes what was done for the
/// session, at once and without blocking: a walk running for it is cancelled, and the undo runs
/// on a thread of the launchers'. Its launch takes it by value, so it is never used twice.
pub struct Reservation {
    matter: String,
    session: String,
    ports: Vec<u16>,
    slot: Option<u8>,
    pool_user: Option<PoolUser>,
    claim: Option<Claim>,
}

impl Reservation {
    /// A reservation with no slot, no ports and no account, which nothing releases: macOS's,
    /// where the jail needs neither.
    pub fn unpooled(matter: &str, session: &str) -> Reservation {
        Reservation {
            matter: matter.to_owned(),
            session: session.to_owned(),
            ports: Vec::new(),
            slot: None,
            pool_user: None,
            claim: None,
        }
    }

    /// A reservation of `ports` with no account, which nothing releases: the host's fake
    /// launchers'.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixed(ports: Vec<u16>, matter: &str, session: &str) -> Reservation {
        Reservation {
            ports,
            ..Reservation::unpooled(matter, session)
        }
    }

    /// One a set of launchers handed out: `claim` releases it when dropped.
    pub(crate) fn claimed(
        matter: &str,
        session: &str,
        slot: (u8, Vec<u16>),
        pool_user: PoolUser,
        claim: Claim,
    ) -> Reservation {
        Reservation {
            matter: matter.to_owned(),
            session: session.to_owned(),
            ports: slot.1,
            slot: Some(slot.0),
            pool_user: Some(pool_user),
            claim: Some(claim),
        }
    }

    /// The matter it is for (`MatterId`'s text form).
    pub fn matter(&self) -> &str {
        &self.matter
    }

    /// The session it is for (the run folder's session name).
    pub fn session(&self) -> &str {
        &self.session
    }

    /// The slot's host ports (`H_s`), which the host binds its proxy's listener on
    /// (`ListenerFor::TierB`) before it builds the policy; empty when there is no slot.
    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    /// The slot's number, when it has one.
    pub fn slot(&self) -> Option<u8> {
        self.slot
    }

    /// The matter's account, when it has one: opaque to the host, which only passes the
    /// reservation back.
    pub fn pool_user(&self) -> Option<&PoolUser> {
        self.pool_user.as_ref()
    }

    /// Which launchers handed it out and under which id, if any did.
    pub(crate) fn claim_id(&self) -> Option<(u64, u64)> {
        self.claim.as_ref().map(|claim| (claim.owner, claim.id))
    }

    /// Its claim, armed, for the launcher `launch` hands out: it releases the session if no
    /// agent ever starts.
    pub(crate) fn take_claim(mut self) -> Option<Claim> {
        self.claim.take()
    }

    /// Lets go of its claim without releasing it: the caller releases it itself.
    pub(crate) fn disarm(mut self) {
        if let Some(mut claim) = self.claim.take() {
            claim.release = None;
        }
    }
}

impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reservation")
            .field("matter", &self.matter)
            .field("session", &self.session)
            .field("ports", &self.ports)
            .field("slot", &self.slot)
            .field("pool_user", &self.pool_user.as_ref().map(|user| &user.name))
            .field("claimed", &self.claim.is_some())
            .finish()
    }
}

/// What releases a claim: the launchers' session registry.
pub(crate) trait Release: Send + Sync {
    /// Releases claim `id` and undoes what was done for its session, unless its agent started
    /// or its end began.
    /// Called from a drop, so it never blocks: no file call, no wait, no lock held across
    /// either; work that blocks goes elsewhere.
    fn release(self: Arc<Self>, id: u64);
}

/// A set of launchers' hold on a reservation; released when dropped, unless disarmed.
pub(crate) struct Claim {
    /// The launchers that handed it out (their registry's own number).
    pub(crate) owner: u64,
    /// Its id in their registry.
    pub(crate) id: u64,
    release: Option<Arc<dyn Release>>,
}

impl Claim {
    /// Claim `id` of the launchers numbered `owner`, released through `release`.
    pub(crate) fn new(owner: u64, id: u64, release: Arc<dyn Release>) -> Claim {
        Claim {
            owner,
            id,
            release: Some(release),
        }
    }

    /// Lets go of it without releasing: the caller releases it itself.
    pub(crate) fn disarm(mut self) {
        self.release = None;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release.release(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Released(Mutex<Vec<u64>>);

    impl Release for Released {
        fn release(self: Arc<Self>, id: u64) {
            self.0.lock().unwrap().push(id);
        }
    }

    fn user() -> PoolUser {
        PoolUser {
            name: "ClauseoSb01".to_owned(),
            sid: crate::windows::sids::Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1001")
                .unwrap(),
        }
    }

    #[test]
    fn unpooled_and_fixed_hold_nothing() {
        let mac = Reservation::unpooled("m", "s");
        assert_eq!((mac.matter(), mac.session()), ("m", "s"));
        assert_eq!(mac.ports(), [0u16; 0]);
        assert_eq!((mac.slot(), mac.claim_id()), (None, None));
        assert!(mac.pool_user().is_none());
        let fake = Reservation::fixed(vec![46100, 46101], "m", "s");
        assert_eq!(fake.ports(), [46100, 46101]);
        assert!(fake.pool_user().is_none());
    }

    #[test]
    fn a_dropped_claim_releases_once_and_a_disarmed_one_never() {
        let released = Arc::new(Released::default());
        let claim = Claim::new(7, 1, Arc::<Released>::clone(&released));
        let held = Reservation::claimed("m", "s", (2, vec![46104]), user(), claim);
        assert_eq!(held.claim_id(), Some((7, 1)));
        assert_eq!((held.slot(), held.ports()), (Some(2), &[46104][..]));
        let debug = format!("{held:?}");
        assert!(
            debug.contains("ClauseoSb01") && !debug.contains("S-1-5"),
            "{debug}"
        );
        drop(held);
        let launched = Reservation::claimed(
            "m",
            "s",
            (2, vec![46104]),
            user(),
            Claim::new(7, 2, Arc::<Released>::clone(&released)),
        );
        launched.disarm();
        assert_eq!(*released.0.lock().unwrap(), [1]);
    }
}
