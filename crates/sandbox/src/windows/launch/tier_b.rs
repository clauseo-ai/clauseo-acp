//! The launcher `Launchers::launch` returns: one agent each, started through the outer runner.
//!
//! TODO(C4): this is C3's minimal one. C4 adds the outer's stderr tap into `denial_from_line`
//! and the denial sink, the minimal environment and `opts.clear_env()`, the 5 s `stdin_grace`
//! (C1), the exit decode (`RunOutcome`) and the end sequence; and decides whether a launch file
//! written for a spawn that failed is removed here (the undo below doesn't remove it).
//!
//! **The claim's hand-over:** `Launchers::launch` gives this launcher the reservation's claim,
//! armed. It is let go of only once the outer has started (from then on `after_session` ends
//! the session). If writing the launch file or starting the outer fails, the session is undone
//! here before the error returns (the run's group, the slot, the session's mark) and the matter
//! freed; if the launcher is dropped unused, the claim's drop does the same, off the dropping
//! thread. C4 keeps this hand-over wherever it moves the start.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions};

use super::registry::Registry;
use super::steps::LaunchSteps;
use crate::reservation::Claim;
use crate::windows::pool::PoolUser;
use crate::{Error, Policy};

/// Starts one agent as the matter's pool user: writes the protected launch file, then starts
/// `clauseo-sandbox-run.exe --launch <file>` in the host's tree.
pub struct TierBLauncher<S: LaunchSteps> {
    registry: Arc<Registry<S>>,
    /// The session's claim, until its agent starts: released (the session undone) if it never
    /// does.
    claim: Mutex<Option<Claim>>,
    policy: Policy,
    pool: PoolUser,
    opts: SpawnOptions,
    started: AtomicBool,
}

impl<S: LaunchSteps> TierBLauncher<S> {
    /// The launcher of the launched session `claim` names, in `registry`.
    pub(super) fn new(
        registry: Arc<Registry<S>>,
        claim: Claim,
        policy: Policy,
        pool: PoolUser,
        opts: SpawnOptions,
    ) -> TierBLauncher<S> {
        TierBLauncher {
            registry,
            claim: Mutex::new(Some(claim)),
            policy,
            pool,
            opts,
            started: AtomicBool::new(false),
        }
    }

    /// Writes the launch file and starts the outer.
    fn start(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let steps = &*self.registry.steps;
        let file = steps
            .write_launch(&self.policy, &self.pool, spec)
            .map_err(|error| {
                if let Error::Launch(error) = error {
                    error
                } else {
                    clauseo_process::Error::Os {
                        what: "the sandbox couldn't write the launch file",
                        source: std::io::Error::other(error.kind()),
                    }
                }
            })?;
        steps.spawn_outer(&file, &self.opts)
    }
}

impl<S: LaunchSteps> Launcher for TierBLauncher<S> {
    /// Starts the agent `spec` names; a second call is refused: each launcher starts one agent.
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(clauseo_process::Error::InvalidSpec(
                "a tier B launcher starts one agent".to_owned(),
            ));
        }
        let started = self.start(spec);
        let claim = self
            .claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(claim) = claim {
            let id = claim.id;
            claim.disarm();
            if started.is_err() {
                // No agent: the session is undone and the matter freed before the error returns.
                self.registry.release_now(id);
            }
        }
        started
    }
}

impl<S: LaunchSteps> std::fmt::Debug for TierBLauncher<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TierBLauncher")
            .field("pool", &self.pool.name)
            .field("started", &self.started.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}
