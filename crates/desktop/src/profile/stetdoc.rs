//! stetdoc in the launch profile (`clauseo_profile::skills::stetdoc::start`): the first check
//! on the host-start thread, and, when its `--version` only passed the limit, one more check on
//! a thread of its own whose answer goes into [`Live`] for the next agent start.
//!
//! Only the profile itself (the instructions' stetdoc line and the skills) changes after the
//! start. stetdoc's folder on the agents' `PATH` and the Word blackline's program are given to
//! the host once, at its start (`with_profile`): after a background check that fails for real,
//! they stay until the app restarts, and the agents are no longer told about stetdoc.
//!
//! Each hook call reads the profile as it is then, and one launch makes several calls (its
//! skills writes on the launch-prep thread, then its `_meta` and Codex keys). So once per
//! session, when the background check's answer lands between them, within its 30 s window, a
//! launch may mix the two: a Codex launch may get the new skills index over the files the old
//! profile wrote. The next launch is consistent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::ClauseoProfile;
use clauseo_profile::skills::stetdoc::start::{self, Start, State};

/// The rendered profile the hook reads at each call, put in place whole when the background
/// check ends. A read takes the lock only to clone an `Arc`; the profile is rendered before
/// the write takes it, so no I/O or rendering happens under it.
#[derive(Debug, Clone)]
pub(crate) struct Live(Arc<RwLock<Arc<ClauseoProfile>>>);

impl Live {
    /// A cell holding `profile`.
    pub(crate) fn new(profile: ClauseoProfile) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(profile))))
    }

    /// The profile as it is now.
    pub(crate) fn get(&self) -> Arc<ClauseoProfile> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Puts `profile` in place for every later [`Self::get`]. The old one is dropped after the
    /// lock is released.
    fn set(&self, profile: ClauseoProfile) {
        let profile = Arc::new(profile);
        let old = {
            let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
            std::mem::replace(&mut *held, profile)
        };
        drop(old);
    }
}

/// The first check of `program`, within `start::FIRST_LIMIT`, with `cached` as the skills
/// folder a previous launch wrote. Starts stetdoc and reads files: on the host-start thread.
pub(super) fn first(program: &Path, cached: &Path) -> Start {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    async_io::block_on(start::start(&launcher, &AsyncIoTimer, program, cached))
}

/// `base` (the profile with no skills yet) with stetdoc as `state` says, and the browser's
/// skill when `browser`. A skill the renderer refuses is left out with a warning.
pub(super) fn render(base: ClauseoProfile, state: &State, browser: bool) -> ClauseoProfile {
    let (profile, refused) = base.with_stetdoc(state, browser);
    for err in refused {
        tracing::warn!(%err, "a skill is left out of the profile");
    }
    profile
}

/// What the background check needs: the program and the cached skills folder, the state the
/// agents start with, and how to render the profile again.
pub(super) struct Recheck {
    pub(super) program: PathBuf,
    pub(super) cached: PathBuf,
    pub(super) state: State,
    pub(super) base: ClauseoProfile,
    pub(super) browser: bool,
}

/// Starts the background check on a thread of its own, within `start::RECHECK_LIMIT`, and puts
/// its answer in `live` when it changes the state. Never waited for: nothing joins the thread,
/// so the app's shutdown does not wait on it, and the process's exit ends it. A stetdoc still
/// running then goes with its Job Object on Windows; on macOS and Linux it is left to finish
/// its one short run. A thread that does not start leaves stetdoc as the first check left it.
pub(super) fn recheck_later(recheck: Recheck, live: Live) {
    let spawned = std::thread::Builder::new()
        .name("stetdoc-recheck".to_owned())
        .spawn(move || {
            let Recheck {
                program,
                cached,
                state,
                base,
                browser,
            } = recheck;
            let launcher = PassthroughLauncher::new(SpawnOptions::new());
            let next = async_io::block_on(start::recheck(
                &launcher,
                &AsyncIoTimer,
                &program,
                &cached,
                &state,
            ));
            if let Some(state) = next {
                live.set(render(base, &state, browser));
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "stetdoc's background check did not start; it stays on as it is");
    }
}
