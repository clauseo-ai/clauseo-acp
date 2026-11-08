//! The launcher `Launchers::launch` returns: one agent each, started through the outer runner.
//!
//! TODO(C4): this is C3's minimal one. C4 adds the outer's stderr tap into `denial_from_line`
//! and the denial sink, the minimal environment and `opts.clear_env()`, the 5 s `stdin_grace`
//! (C1), the exit decode (`RunOutcome`) and the end sequence; and decides whether a launch file
//! written for a spawn that failed is removed here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions};

use super::steps::LaunchSteps;
use crate::windows::pool::PoolUser;
use crate::{Error, Policy};

/// Starts one agent as the matter's pool user: writes the protected launch file, then starts
/// `clauseo-sandbox-run.exe --launch <file>` in the host's tree.
pub struct TierBLauncher<S: LaunchSteps> {
    steps: Arc<S>,
    policy: Policy,
    pool: PoolUser,
    opts: SpawnOptions,
    started: AtomicBool,
}

impl<S: LaunchSteps> TierBLauncher<S> {
    pub(super) fn new(
        steps: Arc<S>,
        policy: Policy,
        pool: PoolUser,
        opts: SpawnOptions,
    ) -> TierBLauncher<S> {
        TierBLauncher {
            steps,
            policy,
            pool,
            opts,
            started: AtomicBool::new(false),
        }
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
        let file = self
            .steps
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
        self.steps.spawn_outer(&file, &self.opts)
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
