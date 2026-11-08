//! Tier B's launcher (S15 PR6): how the host starts an agent as its matter's pool user. The
//! host's `SandboxLaunchers` seam (#660) wraps it, in desktop; the sandbox takes no host types,
//! so matters and sessions are their stable text forms (`MatterId`'s `Display`, the run folder's
//! session name). 0049's "The launcher (PR6 interface)" is the contract.
//!
//! The host's calls, in order (one live agent per matter):
//!
//! 1. [`Launchers::reserve`]: quick bookkeeping, within the launch bound. The session is marked
//!    open, the matter's account assigned, a network slot claimed. Its [`Reservation`] gives the
//!    slot's ports, which the host binds its proxy on before it builds the policy.
//! 2. [`Launchers::prepare`] with the reservation and the full policy: the background walk
//!    ([`Preparing`], `prepare.rs`), which hands the reservation back prepared. A later launch
//!    takes 20-70 ms here; a first one 17-30 s.
//! 3. [`Launchers::launch`] with it, by value: the launcher that starts the outer runner.
//! 4. [`Launchers::after_session`], once the agent's process tree is gone: the repair, the
//!    set-aside's pass, the clean-close record, `EndRun`. Only then is the matter free.
//!
//! A reservation dropped before its launch undoes what was done for the session, newest first:
//! the run's group, the slot, the session's mark. The matter keeps its account (no lease;
//! `MatterSids::release` gives it back on forget). Every OS effect is a [`LaunchSteps`] call,
//! so the order is tested on every OS with fakes (`launch/tests`); the real steps are C5's.

use std::sync::Arc;

use clauseo_process::SpawnOptions;

mod prepare;
mod registry;
mod steps;
mod tier_b;

pub use prepare::{PREPARE_BOUND, Preparing, Progress, RETRY_PAUSE, Stage, Walk};
pub use steps::{Ending, LaunchSteps, RunRefusal, Slot, Stopped};
pub use tier_b::TierBLauncher;

use registry::{Registry, State};

use crate::reservation::{Claim, Release};
use crate::windows::pool::PoolFullCause;
use crate::windows::pool::assign::Assign;
use crate::{DenialSink, Error, Policy, Reservation, SetAside, SetupError};

/// Tier B's launchers over `S`'s steps, with the registry of their sessions.
pub struct Launchers<S: LaunchSteps> {
    registry: Arc<Registry<S>>,
}

impl<S: LaunchSteps> Launchers<S> {
    /// Launchers over `steps`. First clears what a crashed app left
    /// ([`LaunchSteps::clear_stale`]): stale launch files, slot claims and session marks.
    ///
    /// TODO(C5): `Launchers::new(data_dir, runner)`, `#[cfg(windows)]`, is this over `OsSteps`.
    pub fn with_steps(steps: S) -> Result<Launchers<S>, Error> {
        steps.clear_stale()?;
        Ok(Launchers {
            registry: Arc::new(Registry::new(Arc::new(steps))),
        })
    }

    /// Reserves the matter's account and a network slot for `session`; `running` names the
    /// matter's other sessions the host still runs (none, on tier B; a session marked open and
    /// not named crashed). Bookkeeping only: nothing runs as the pool user and nothing is
    /// granted to it. [`Error::MatterBusy`] while another session holds the matter, until its
    /// [`after_session`](Launchers::after_session) has finished.
    pub fn reserve(
        &self,
        matter: &str,
        session: &str,
        running: &[&str],
    ) -> Result<Reservation, Error> {
        let registry = &self.registry;
        let steps = &*registry.steps;
        let id = registry.begin(matter, session, running)?;
        let release: Arc<dyn Release> = Arc::<Registry<S>>::clone(registry);
        // Dropped on any failure below, which undoes what was done.
        let claim = Claim::new(registry.owner, id, release);
        steps.setup_ready()?;
        steps.open_session(matter, session, running)?;
        registry.update(matter, id, |entry| entry.opened = true);
        let pool = steps.assign(matter, session).map_err(assign_refused)?;
        registry.update(matter, id, |entry| entry.pool = Some(pool.clone()));
        let slot = steps.claim_slot(matter, &pool)?;
        registry.update(matter, id, |entry| {
            entry.slot = Some(slot.clone());
            entry.state = State::Reserved;
        });
        Ok(Reservation::claimed(
            matter,
            session,
            (slot.index, slot.ports),
            pool,
            claim,
        ))
    }

    /// Starts the matter's preparation on `reservation` (see `prepare.rs`), and returns at
    /// once. A reservation these launchers don't hold as reserved fails it with
    /// [`Error::WrongReservation`].
    pub fn prepare(&self, reservation: Reservation, policy: &Policy) -> Preparing {
        let Some(id) = self.own(&reservation) else {
            return Preparing::failed(Some(reservation), Error::WrongReservation);
        };
        let (matter, session) = (reservation.matter(), reservation.session());
        if !self
            .registry
            .step(matter, session, id, State::Reserved, State::Preparing)
        {
            return Preparing::failed(Some(reservation), Error::WrongReservation);
        }
        Preparing::start(Arc::clone(&self.registry), id, reservation, policy.clone())
    }

    /// The launcher for `session` of `matter` on its prepared `reservation`: what the launch
    /// pass moved comes with it, for the lawyer's notice. Refused with
    /// [`Error::WrongReservation`] for a reservation of another matter or session, or of other
    /// launchers, or a policy for another matter folder than the one prepared; with
    /// [`Error::NotPrepared`] for one not prepared (the host reserves and prepares again). A
    /// refused reservation is released, and so is one whose pool user has a process running
    /// ([`Error::LeftoverProcess`]). The seam's `running` went to `reserve`. `denials` is
    /// unused until C4 feeds it the outer's denial lines.
    pub fn launch(
        &self,
        reservation: Reservation,
        policy: Policy,
        matter: &str,
        session: &str,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
    ) -> Result<(Arc<dyn clauseo_process::Launcher>, Vec<SetAside>), Error> {
        let id = self.own(&reservation).ok_or(Error::WrongReservation)?;
        if reservation.matter() != matter || reservation.session() != session {
            return Err(Error::WrongReservation);
        }
        let pool = self
            .registry
            .prepared(matter, session, id, policy.matter())?;
        // Nothing runs as the pool user between the preparation and here; a process that does
        // is refused plainly (the preparation stops leftovers, within its bound).
        let leftovers = self.registry.steps.pool_user_processes(&pool)?;
        if !leftovers.is_empty() {
            return Err(Error::LeftoverProcess {
                count: leftovers.len(),
            });
        }
        let (pool, moved) = self.registry.launch(matter, session, id, policy.matter())?;
        // The registry holds the session from here until `after_session`.
        reservation.disarm();
        // TODO(C4): the outer's stderr lines go to `denials` through `denial_from_line`.
        drop(denials);
        let launcher = TierBLauncher::new(Arc::clone(&self.registry.steps), policy, pool, opts);
        Ok((Arc::new(launcher), moved))
    }

    /// Once per launched agent, after its process tree is gone: in order the journal's cursor,
    /// the repair, the end of R18's reads, the set-aside's pass, the clean-close record and
    /// `EndRun`; then the slot goes back and the matter is free. What the pass moved. A session
    /// these launchers didn't launch is left alone (the next launch's set-aside pass checks
    /// it).
    pub fn after_session(&self, matter: &str, session: &str, policy: &Policy) -> Vec<SetAside> {
        let registry = &self.registry;
        let steps = &*registry.steps;
        let Some(ended) = registry.end(matter, session) else {
            tracing::warn!("tier B's after-session pass for a session it didn't launch");
            return Vec::new();
        };
        let mut moved = Vec::new();
        if let Some(pool) = &ended.pool {
            let end = Ending {
                matter,
                session,
                policy,
                pool,
            };
            let cursor = steps.session_cursor(&end);
            let repaired = steps.repair(&end);
            if let Err(error) = &repaired {
                tracing::warn!(
                    kind = error.kind(),
                    "tier B's repair after the session failed"
                );
            }
            steps.end_reads(&end);
            let passed = steps.set_aside_after(&end);
            match &passed {
                Ok(passed) => moved = steps.moved(passed),
                Err(error) => {
                    tracing::warn!(
                        kind = error.kind(),
                        "tier B's set-aside after the session failed"
                    );
                }
            }
            steps.write_clean_close(&end, cursor.as_ref(), &repaired, &passed);
        }
        if let Some(run) = ended.run {
            steps.end_run(run);
        }
        if let Some(slot) = &ended.slot {
            steps.release_slot(slot);
        }
        registry.finish(matter, ended.id);
        moved
    }

    /// The id of `reservation` in these launchers' registry, if they handed it out.
    fn own(&self, reservation: &Reservation) -> Option<u64> {
        reservation
            .claim_id()
            .filter(|&(owner, _)| owner == self.registry.owner)
            .map(|(_, id)| id)
    }
}

impl<S: LaunchSteps> std::fmt::Debug for Launchers<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launchers").finish_non_exhaustive()
    }
}

/// `assign`'s refusal as the launch's error: no free account is [`Error::PoolFull`] with its
/// counts; another session open is [`Error::MatterBusy`]; records that can't be read are
/// [`Error::Probe`], a block [`Error::Blocked`].
pub(crate) fn assign_refused(refused: Assign) -> Error {
    match refused {
        Assign::PoolFull { size, held_back } => Error::PoolFull {
            cause: PoolFullCause::of(size, &held_back),
        },
        Assign::Busy(_) => Error::MatterBusy,
        Assign::Records(SetupError::Blocked(blocked)) => Error::Blocked(blocked),
        Assign::Records(error @ (SetupError::Refused | SetupError::Failed(_))) => {
            Error::Probe(std::io::Error::other(error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests;
