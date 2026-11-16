//! stetdoc in the launch profile (`clauseo_profile::skills::stetdoc::start`): on the host-start
//! thread, one 3 s try for a stetdoc seen to answer before and none for one not seen yet (the
//! OS may be scanning it); when the start has no answer, the background tries (20 s for one not
//! seen yet, then 20 s, then 30 s) on a thread of their own, whose answer goes into [`Live`] for
//! the agent starts that follow. An agent launch while a try is in flight waits for that try's
//! end, at most 20 s from the launch (`ProfileHook::launch_wait`), with its turn "starting"
//! meanwhile. An agent already running when the answer lands is marked to relaunch
//! (`ProfileHook::changed`): the next question in its conversation starts it again with the
//! skill, and with none it stops at the idle limit, never mid-turn. Nothing is shown to the
//! lawyer.
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
//!
//! It also finds the stetdoc to run: the bundled one ([`bundled_stetdoc`]), or in a debug
//! build the first on `PATH` ([`stetdoc_program`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_host::ProfileChange;
use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::ClauseoProfile;
use clauseo_profile::skills::stetdoc::start::{self, InFlight, Later, Limits, Start, State};
use futures::channel::oneshot;

use super::{STETDOC, first_on_path, is_program};

/// The rendered profile the hook reads at each call, put in place whole when the background
/// check ends, with its generation (0 at the start, one more at each change) and the waits for
/// the next change (`ProfileHook::changed`). A read takes the lock only to clone an `Arc`; the
/// profile is rendered before the write takes it, so no I/O or rendering happens under it.
#[derive(Debug, Clone)]
pub(crate) struct Live(Arc<Mutex<Held>>);

/// What [`Live`] holds.
#[derive(Debug)]
struct Held {
    generation: u64,
    profile: Arc<ClauseoProfile>,
    waits: Vec<oneshot::Sender<u64>>,
}

impl Live {
    /// A cell holding `profile`, generation 0.
    pub(crate) fn new(profile: ClauseoProfile) -> Self {
        Self(Arc::new(Mutex::new(Held {
            generation: 0,
            profile: Arc::new(profile),
            waits: Vec::new(),
        })))
    }

    /// The profile as it is now.
    pub(crate) fn get(&self) -> Arc<ClauseoProfile> {
        self.now().1
    }

    /// The generation and the profile as they are now, read together.
    pub(crate) fn now(&self) -> (u64, Arc<ClauseoProfile>) {
        let held = self.lock();
        (held.generation, Arc::clone(&held.profile))
    }

    /// A wait that ends with the generation once it is past `seen`: at once when it is already.
    /// A cell dropped first (the app ends) leaves the wait pending.
    pub(crate) fn changed(&self, seen: u64) -> ProfileChange {
        let receiver = {
            let mut held = self.lock();
            if held.generation > seen {
                let generation = held.generation;
                return Box::pin(async move { generation });
            }
            let (sender, receiver) = oneshot::channel();
            held.waits.push(sender);
            receiver
        };
        Box::pin(async move {
            match receiver.await {
                Ok(generation) => generation,
                Err(oneshot::Canceled) => std::future::pending().await,
            }
        })
    }

    /// Puts `profile` in place for every later [`Self::get`] as the next generation, and ends
    /// the waits for a change. The old profile is dropped after the lock is released.
    fn set(&self, profile: ClauseoProfile) {
        let profile = Arc::new(profile);
        let (generation, old, waits) = {
            let mut held = self.lock();
            held.generation += 1;
            let old = std::mem::replace(&mut held.profile, profile);
            (held.generation, old, std::mem::take(&mut held.waits))
        };
        drop(old);
        for wait in waits {
            // A wait nobody holds any more.
            let _gone = wait.send(generation);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Held> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
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
            // Only a real change is put in place: each one marks every agent launched before it to
            // relaunch. stetdoc found broken (`State::Off`) is one, on purpose: those agents are
            // told of a stetdoc that no longer answers, so each relaunches once without it.
            if let Some(new) = answered.state.filter(|new| *new != state) {
                live.set(render(base, &new, browser));
            }
            // The try ends after its answer is in place: a launch that waited reads it.
            drop(answered.held);
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "stetdoc's background check did not start; it stays on as it is");
    }
}

/// Where a release bundles stetdoc, in a folder of its own so that only that folder goes first
/// on the agents' `PATH` (U13; `.github/scripts/sidecars.mjs`, `crates/desktop/stetdoc.toml`).
/// On macOS (`macos`), `Clauseo.app/Contents/Helpers/stetdoc/stetdoc`, found from `exe_dir`
/// (`Contents/MacOS`): notarisation wants no program in `Contents/Resources`. Elsewhere
/// `<resource_dir>/stetdoc/stetdoc[.exe]`, Tauri's resource folder (on Windows the install
/// folder, beside `Clauseo.exe`; on Linux `/usr/lib/Clauseo`). None when that folder is not
/// known.
pub(crate) fn bundled_stetdoc(
    macos: bool,
    exe_dir: &Path,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    let program = format!("{STETDOC}{}", std::env::consts::EXE_SUFFIX);
    let folder = if macos {
        exe_dir.parent()?.join("Helpers")
    } else {
        resource_dir?.to_path_buf()
    };
    Some(folder.join(STETDOC).join(program))
}

/// The stetdoc to run: `bundled` (from [`bundled_stetdoc`]) when it is a program there. When
/// it is not, a `release` build runs none and warns (the profile then leaves stetdoc out), and
/// a debug build takes the first `stetdoc` on `path` (the `PATH` value).
pub(crate) fn stetdoc_program(
    bundled: Option<&Path>,
    path: Option<&OsStr>,
    release: bool,
) -> Option<PathBuf> {
    if let Some(bundled) = bundled.filter(|bundled| is_program(bundled)) {
        return Some(bundled.to_path_buf());
    }
    if release {
        tracing::warn!(
            expected = ?bundled,
            "the bundled stetdoc is missing: agents start without it"
        );
        return None;
    }
    first_on_path(STETDOC, path?)
}
