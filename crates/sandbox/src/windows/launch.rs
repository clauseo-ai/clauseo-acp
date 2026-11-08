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
//!    [`Error::MatterBusy`] means retry within the bound.
//! 2. [`Launchers::prepare`] with the reservation, borrowed, and the full policy: the background
//!    walk ([`Preparing`], `prepare.rs`), keyed by the reservation's claim. A later launch
//!    takes 20-70 ms here; a first one 17-30 s.
//! 3. [`Launchers::launch`] with it, by value: the launcher that starts the outer runner. The
//!    launcher holds the reservation's claim until its agent starts: if writing the launch file
//!    or starting the outer fails, or it is dropped unused, it undoes the session and frees the
//!    matter itself.
//! 4. [`Launchers::after_session`], once the agent's process tree is gone: the repair, the
//!    set-aside's pass, the clean-close record, `EndRun`. Only then is the matter free, even if
//!    a step panics (drop guards end the run and free it).
//!
//! The host keeps one owner of the reservation from `reserve` through `launch`. Dropped before
//! its launch, it undoes what was done for the session, newest first (the run's group, the
//! slot, the session's mark), and cancels a walk running for it; the drop itself never blocks
//! (`registry.rs`). The matter keeps its account (no lease;
//! `MatterSids::release` gives it back on forget). Every OS effect is a [`LaunchSteps`] call,
//! so the order is tested on every OS with fakes (`launch/tests`); the real steps are C5's.
//!
//! **The host's 180 s bound is the overall one** and covers `reserve` through `launch`
//! (D3b-win): its `MatterBusy` retries, the walk's waits and the launch. The walk's own limits
//! (`walk.rs`: the waits' 180 s budget, the work's 60 s stall timer and 20 min cap) only bound
//! the walk inside it; a large matter's first walk keeps going while its progress moves, and
//! the host shows it.
//!
//! One set of launchers per sandbox folder, across processes: [`Launchers::with_steps`] takes
//! the folder's lock ([`LAUNCHERS_LOCK`]) before it clears what a crash left, and a second set
//! (another Clauseo) is refused at once with [`Error::LaunchersHeld`].

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::SpawnOptions;

mod prepare;
mod registry;
mod steps;
mod tier_b;
mod walk;

pub use prepare::{
    PREPARE_BOUND, PREPARE_STALL, Preparing, Progress, RETRY_PAUSE, STUCK_GRACE, Stage,
    WORK_CEILING, Walk,
};
pub use steps::{Ending, LaunchSteps, RunRefusal, Slot, Stopped};
pub use tier_b::TierBLauncher;

use prepare::Reserved;
use registry::{Begun, FinishEnd, Registry, State};

use crate::reservation::{Claim, Release};
use crate::windows::pool::assign::Assign;
use crate::windows::pool::{PoolFullCause, PoolUser};
use crate::windows::write;
use crate::{DenialSink, Error, Policy, Reservation, SetAside, SetupError};

/// The lock file in the sandbox's data folder ([`LaunchSteps::data_dir`]) that one set of
/// launchers holds for as long as it lives.
pub const LAUNCHERS_LOCK: &str = "tier-b-launchers.lock";

/// Tier B's launchers over `S`'s steps, with the registry of their sessions.
pub struct Launchers<S: LaunchSteps> {
    registry: Arc<Registry<S>>,
}

impl<S: LaunchSteps> Launchers<S> {
    /// Launchers over `steps`. First takes the sandbox folder's lock ([`LAUNCHERS_LOCK`]),
    /// without waiting: held by other launchers, in this process or another Clauseo, it is
    /// [`Error::LaunchersHeld`] and nothing is cleared (the stricter choice: a wait would only
    /// help a Clauseo that is quitting, whose lock goes with its process). Then, with the lock
    /// held, clears what a crashed app left ([`LaunchSteps::clear_stale`]): stale launch files,
    /// slot claims and session marks. The lock lasts until the launchers, every reservation and
    /// every walk and undo of theirs are gone.
    ///
    /// TODO(C5): `Launchers::new(data_dir, runner)`, `#[cfg(windows)]`, is this over `OsSteps`.
    pub fn with_steps(steps: S) -> Result<Launchers<S>, Error> {
        let data_lock = lock_data(steps.data_dir())?;
        steps.clear_stale()?;
        Ok(Launchers {
            registry: Arc::new(Registry::new(Arc::new(steps), data_lock)),
        })
    }

    /// Reserves the matter's account and a network slot for `session`; `running` names the
    /// matter's other sessions the host still runs (none, on tier B; a session marked open and
    /// not named crashed). Bookkeeping only: nothing runs as the pool user and nothing is
    /// granted to it (the slot's claim adds no firewall permit). A refusal undoes what was done
    /// before it returns.
    ///
    /// **[`Error::MatterBusy`] means retry** (one live agent per matter): another session holds
    /// the matter, from its reservation until its `after_session` has finished, or a released
    /// reservation's undo is still running, or the previous session's detached end sweeper is
    /// still running as the pool user ([`LaunchSteps::end_sweeper_running`]). The host retries
    /// within its 180 s bound, showing that it is preparing. `reserve` never returns
    /// [`Error::LeftoverProcess`]. **[`Error::MatterStuck`] is final** until Clauseo restarts: a
    /// step for the matter never returned ([`STUCK_GRACE`]). While the pool user's last
    /// `EndRun` keeps failing, it is [`Error::PoolFull`] with the account needing a reset.
    pub fn reserve(
        &self,
        matter: &str,
        session: &str,
        running: &[&str],
    ) -> Result<Reservation, Error> {
        let registry = &self.registry;
        let id = registry.begin(matter, session, running)?;
        let release: Arc<dyn Release> = Arc::<Registry<S>>::clone(registry);
        let claim = Claim::new(registry.owner, id, release);
        match self.reserve_steps(matter, session, running, id) {
            Ok((pool, slot)) => Ok(Reservation::claimed(
                matter,
                session,
                (slot.index, slot.ports),
                pool,
                claim,
            )),
            Err(error) => {
                // Undone here, before the refusal returns.
                claim.disarm();
                registry.release_now(id);
                Err(error)
            }
        }
    }

    /// `reserve`'s steps for claim `id`, each recorded for the undo.
    fn reserve_steps(
        &self,
        matter: &str,
        session: &str,
        running: &[&str],
        id: u64,
    ) -> Result<(PoolUser, Slot), Error> {
        let registry = &self.registry;
        let steps = &*registry.steps;
        steps.setup_ready()?;
        steps.open_session(matter, session, running)?;
        registry.update(matter, id, |entry| entry.opened = true);
        let pool = steps.assign(matter, session).map_err(assign_refused)?;
        registry.update(matter, id, |entry| entry.pool = Some(pool.clone()));
        // A failed `EndRun` of the pool user's last run is tried again; while it fails, the
        // account needs a reset and no run reuses it.
        registry.retry_owed(&pool)?;
        if steps.end_sweeper_running(&pool) {
            return Err(Error::MatterBusy);
        }
        let slot = steps.claim_slot(matter, &pool, id)?;
        registry.update(matter, id, |entry| {
            entry.slot = Some(slot.clone());
            entry.state = State::Reserved;
        });
        Ok((pool, slot))
    }

    /// Starts the matter's preparation for `reservation` (see `prepare.rs`), and returns at
    /// once; the host keeps the reservation. The walk is keyed by its claim and matter: a second
    /// call while it runs joins it, and one once it is prepared finishes at once. A reservation
    /// these launchers don't hold is [`Error::WrongReservation`]; one whose walk failed is
    /// [`Error::NotPrepared`] (the host drops it and reserves again). Dropping the reservation
    /// cancels its walk.
    pub fn prepare(&self, reservation: &Reservation, policy: &Policy) -> Preparing {
        let Some(id) = self.own(reservation) else {
            return Preparing::finished(Err(Error::WrongReservation));
        };
        let (Some(pool), Some(index)) = (reservation.pool_user(), reservation.slot()) else {
            return Preparing::finished(Err(Error::WrongReservation));
        };
        let (matter, session) = (reservation.matter(), reservation.session());
        match self.registry.begin_walk(matter, session, id) {
            Ok(Begun::Start(shared)) => {
                let reserved = Reserved {
                    id,
                    matter: matter.to_owned(),
                    session: session.to_owned(),
                    pool: pool.clone(),
                    slot: Slot {
                        index,
                        ports: reservation.ports().to_vec(),
                        claim: id,
                    },
                };
                Preparing::start(&self.registry, reserved, policy.clone(), shared)
            }
            Ok(Begun::Join(shared)) => Preparing::joined(shared),
            Ok(Begun::Done) => Preparing::finished(Ok(())),
            Err(error) => Preparing::finished(Err(error)),
        }
    }

    /// The launcher for `session` of `matter` on its prepared `reservation`: what the launch
    /// pass moved comes with it, for the lawyer's notice. Refused with
    /// [`Error::WrongReservation`] for a reservation of another matter or session, or of other
    /// launchers, or a policy for another matter folder than the one prepared; with
    /// [`Error::NotPrepared`] for one not prepared, or whose walk failed (the host reserves and
    /// prepares again). A refused reservation is released before the refusal returns.
    ///
    /// **[`Error::LeftoverProcess`] here is final:** the preparation stopped, swept and looked
    /// again within its bound, so a process of the pool user's now started in the gap. The host
    /// doesn't retry; it is a plain launch failure. The seam's `running` went to `reserve`.
    /// `denials` is unused until C4 feeds it the outer's denial lines.
    ///
    /// The launcher holds the reservation's claim until its agent starts ([`TierBLauncher`]):
    /// a failed start, or a launcher dropped unused, undoes the session and frees the matter,
    /// so the host's `after_session` then finds nothing to do.
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
        let launched = self.launch_checked(&reservation, &policy, matter, session, id);
        let (pool, moved) = match launched {
            Ok(launched) => launched,
            Err(error) => {
                reservation.disarm();
                self.registry.release_now(id);
                return Err(error);
            }
        };
        // Launched: the launcher holds the claim until its agent starts, then `after_session`
        // ends the session.
        let Some(claim) = reservation.take_claim() else {
            self.registry.release_now(id);
            return Err(Error::WrongReservation);
        };
        // TODO(C4): the outer's stderr lines go to `denials` through `denial_from_line`.
        drop(denials);
        let launcher = TierBLauncher::new(Arc::clone(&self.registry), claim, policy, pool, opts);
        Ok((Arc::new(launcher), moved))
    }

    /// `launch`'s checks of claim `id`, then the registry's move to launched.
    fn launch_checked(
        &self,
        reservation: &Reservation,
        policy: &Policy,
        matter: &str,
        session: &str,
        id: u64,
    ) -> Result<(PoolUser, Vec<SetAside>), Error> {
        if reservation.matter() != matter || reservation.session() != session {
            return Err(Error::WrongReservation);
        }
        let pool = self
            .registry
            .prepared(matter, session, id, policy.matter())?;
        // Nothing runs as the pool user between the preparation and here: a process that does
        // started in the gap, and is refused plainly.
        let leftovers = self.registry.steps.pool_user_processes(&pool)?;
        if !leftovers.is_empty() {
            return Err(Error::LeftoverProcess {
                count: leftovers.len(),
            });
        }
        self.registry.launch(matter, session, id, policy.matter())
    }

    /// Once per launched agent, after its process tree is gone: in order the journal's cursor,
    /// the repair, the end of R18's reads, the set-aside's pass, the clean-close record and
    /// `EndRun`; then the slot goes back and the matter is free. What the pass moved. A session
    /// these launchers didn't launch is left alone (the next launch's set-aside pass checks
    /// it), and so is one whose launcher already undid it (its agent never started).
    ///
    /// `EndRun`, the slot and the matter's release always run: drop guards do them, so a panic
    /// in a step still ends the run's group and frees the matter before it goes on up.
    ///
    /// The repair and the pass take as long as the matter needs, under the walk's rule: their
    /// counted progress ([`Ending::walk`]) must rise every [`PREPARE_STALL`], within
    /// [`WORK_CEILING`] overall. A step that stalls or passes the cap stops (`Cancelled`) and
    /// the end goes on, the clean-close record noting it failed. A step that doesn't return
    /// makes the matter `MatterStuck` meanwhile; once it does return, the matter is freed.
    pub fn after_session(&self, matter: &str, session: &str, policy: &Policy) -> Vec<SetAside> {
        let registry = &self.registry;
        let steps = &*registry.steps;
        let Some(ended) = registry.end(matter, session) else {
            tracing::warn!("tier B's after-session pass for a session it didn't launch");
            return Vec::new();
        };
        // Dropped in reverse: the run's group and the slot first, then the matter freed.
        let _finish = FinishEnd {
            registry,
            matter,
            id: ended.id,
        };
        let _end_run = EndRun {
            registry,
            pool: ended.pool.clone(),
            run: ended.run,
            slot: ended.slot,
        };
        let mut moved = Vec::new();
        if let Some(pool) = &ended.pool {
            let now = || steps.now();
            let walk = Walk {
                shared: &ended.walk,
                now: &now,
            };
            // Each step's end is progress: a stalled step stops, and the next gets its own time.
            let stepped = || ended.walk.tick(steps.now());
            let end = Ending {
                walk: &walk,
                matter,
                session,
                policy,
                pool,
            };
            let cursor = steps.session_cursor(&end);
            stepped();
            let repaired = steps.repair(&end);
            stepped();
            if let Err(error) = &repaired {
                tracing::warn!(
                    kind = error.kind(),
                    "tier B's repair after the session failed"
                );
            }
            steps.end_reads(&end);
            stepped();
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

/// Ends the run's group, then gives the slot back (unless `EndRun` failed: then both stay held),
/// when dropped: at the end of `after_session`, or on a panic in one of its steps.
struct EndRun<'a, S: LaunchSteps> {
    registry: &'a Registry<S>,
    pool: Option<PoolUser>,
    run: Option<S::RunGroup>,
    slot: Option<Slot>,
}

impl<S: LaunchSteps> Drop for EndRun<'_, S> {
    fn drop(&mut self) {
        let (run, slot) = (self.run.take(), self.slot.take());
        self.registry
            .end_run_and_free_slot(self.pool.as_ref(), run, slot);
    }
}

/// Takes the sandbox folder `dir`'s launchers lock, without waiting and without following a
/// link (checked as the matters lock is, then opened no-follow): [`Error::LaunchersHeld`] while
/// another holder has it, [`Error::Probe`] when the file can't be opened or isn't a plain file.
fn lock_data(dir: &Path) -> Result<File, Error> {
    let path = dir.join(LAUNCHERS_LOCK);
    write::check_owned_file(&path).map_err(Error::Probe)?;
    match write::lock_no_follow_for(&path, Duration::ZERO) {
        Ok(file) => {
            // A lock file the open above just made gets the folder's entries too.
            write::check_owned_file(&path).map_err(Error::Probe)?;
            Ok(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::warn!("tier B's launchers are held by another Clauseo");
            Err(Error::LaunchersHeld)
        }
        Err(error) => Err(Error::Probe(error)),
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
