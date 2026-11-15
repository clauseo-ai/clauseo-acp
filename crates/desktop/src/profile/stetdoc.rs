//! stetdoc in the launch profile (`clauseo_profile::skills::stetdoc::start`): on the host-start
//! thread, one 3 s try for a stetdoc seen to answer before and none for one not seen yet (the
//! OS may be scanning it); when the start has no answer, the background tries (20 s for one not
//! seen yet, then 20 s, then 30 s) on a thread of their own, whose answer goes into [`Live`] for
//! the agent starts that follow. An agent launch while a try is in flight waits for that try's
//! end, at most 20 s from the launch (`ProfileHook::launch_wait`), with its turn "starting"
//! meanwhile. An agent already running keeps the profile it started with: a conversation whose
//! agent started before the answer gets the skill when its agent starts again (after 300 s idle,
//! for one) or in a new conversation. Nothing is shown to the lawyer.
//!
//! Only the profile itself (the instructions' stetdoc line and the skills) changes after the
//! start. stetdoc's folder on the agents' `PATH` and the Word blackline's program are given to
//! the host once, at its start (`with_profile`): after a background check that fails for real,
//! they stay until the app restarts, and the agents are no longer told about stetdoc.
//!
//! Each hook call reads the profile as it is then, and one launch makes several calls (its
//! skills writes on the launch-prep thread, then its `_meta` and Codex keys). So once per
//! session, when the background answer lands between them (within 70 s of the start), a
//! launch may mix the two: a Codex launch may get the new skills index over the files the old
//! profile wrote. The next launch is consistent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::ClauseoProfile;
use clauseo_profile::skills::stetdoc::start::{self, InFlight, Later, Limits, Start, State};

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

/// stetdoc at the start within `limits`, with `cached` as the skills folder a previous launch
/// wrote and `seen` as the marker of the stetdoc seen to answer (`ProfilePaths::stetdoc_seen`).
/// Starts stetdoc and reads files: on the host-start thread, which waits at most `limits.seen`
/// (3 s in the app), and not at all for a stetdoc not seen yet.
pub(super) fn first(program: &Path, cached: &Path, seen: &Path, limits: Limits) -> Start {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    async_io::block_on(start::start(
        &launcher,
        &AsyncIoTimer,
        program,
        cached,
        seen,
        limits,
    ))
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

/// What the background tries need: the first try, marked in flight before the thread starts,
/// the program, the cached skills folder, the marker of the stetdoc seen to answer and the
/// tries' limits, the state the agents start with, and how to render the profile again.
pub(super) struct Recheck {
    pub(super) held: InFlight,
    pub(super) program: PathBuf,
    pub(super) cached: PathBuf,
    pub(super) seen: PathBuf,
    pub(super) later: Vec<Duration>,
    pub(super) state: State,
    pub(super) base: ClauseoProfile,
    pub(super) browser: bool,
}

/// Starts the background tries on a thread of its own, within `Recheck::later`, and puts
/// its answer in `live` when it changes the state. Never waited for: nothing joins the thread,
/// so the app's shutdown does not wait on it, and the process's exit ends it. A stetdoc still
/// running then goes with its Job Object on Windows; on macOS and Linux it is left to finish
/// its one short run. On Windows an exit that lands between stetdoc's suspended start and its
/// joining the job leaves it suspended for good, outside any job, its program file held. The
/// quit's stop closes `clauseo_process`'s gate once the host stopped and waits at most 2 s for
/// its count to reach zero (`quit/settle.rs`); an exit with nothing running closes the gate and
/// reads a zero first (`quit.rs`'s `nothing_runs`). A crash, or a logoff whose wait runs out,
/// is not covered. A test must not end while this thread may run (no stetdoc on its `PATH`).
/// A thread that does not start leaves stetdoc as the first check left it.
pub(super) fn recheck_later(recheck: Recheck, live: Live) {
    let spawned = std::thread::Builder::new()
        .name("stetdoc-recheck".to_owned())
        .spawn(move || {
            let Recheck {
                held,
                program,
                cached,
                seen,
                later,
                state,
                base,
                browser,
            } = recheck;
            let launcher = PassthroughLauncher::new(SpawnOptions::new());
            let later = Later {
                limits: &later,
                held,
            };
            let answered = async_io::block_on(start::recheck(
                &launcher,
                &AsyncIoTimer,
                &program,
                &cached,
                &seen,
                &state,
                later,
            ));
            if let Some(state) = answered.state {
                live.set(render(base, &state, browser));
            }
            // The try ends after its answer is in place: a launch that waited reads it.
            drop(answered.held);
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "stetdoc's background check did not start; it stays on as it is");
    }
}
