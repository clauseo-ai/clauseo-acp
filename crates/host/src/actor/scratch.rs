//! The Scratch matter on the first start. The free folder (`<default_matters_dir>/Scratch`,
//! `Scratch 2`, ...) is found off the actor ([`off_actor`], waited on for [`SCAN_WAIT`]), as
//! each `exists` may hang for long on a Documents folder on a network share that does not
//! answer. The answer comes back as [`ScratchInput::Scanned`], and only then does the actor
//! start the create on the new matter's worker (`matters.rs`).
//!
//! Start-up's ready waits for Scratch (the scan, the create on its worker and the commit) at
//! most [`READY_WAIT`] from when the scan began: past it ready goes out, and a Scratch made later
//! joins the list as any new matter does, with its `MatterCreated` and a `MattersChanged` to app
//! subscribers. Shutdown does not wait for the scan: an answer that comes while the host stops
//! (or once the actor is gone) makes nothing. A create already on its worker is waited for and
//! committed, as a `CreateMatter`'s is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::MatterKind;
use futures::future::AbortHandle;

use super::input::{Input, MatterTag};
use super::{Actor, HostPhase};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::log_file::Cause;

/// The Scratch matter's name.
const SCRATCH: &str = "Scratch";

/// How long start-up's ready waits for the Scratch matter, from when its scan began.
pub(crate) const READY_WAIT: Duration = Duration::from_secs(3);

/// How long the scan is waited for before there is no Scratch matter this run. Nothing waits on
/// it past [`READY_WAIT`], so it is longer than [`Job::short`]'s: a share that is down can take
/// 20-40 s per call, and a scan that answers late still makes Scratch. (Its own length also
/// keeps it apart from the 5 s timers tests look for on the manual clock.)
const SCAN_WAIT: Duration = Duration::from_secs(45);

/// The scan's [`off_actor`] job.
const SCAN_JOB: Job = Job {
    what: "Scratch folder scan",
    matter: None,
    wait: SCAN_WAIT,
};

/// The scan for Scratch's folder: given the default matters folder, the first of `Scratch`,
/// `Scratch 2`, ... that does not exist. [`free_folder`], or a stand-in in tests.
pub(crate) type Scan = Arc<dyn Fn(&Path) -> Option<PathBuf> + Send + Sync>;

/// The Scratch matter's seams in `HostConfig`: the scan, and (tests) whether shutdown waits for
/// it ([`ScratchWait::stop_held`]).
pub(crate) struct Seams {
    /// [`free_folder`], or a test's stand-in.
    pub(crate) scan: Scan,
    /// See [`ScratchWait::stop_held`].
    #[cfg(test)]
    pub(crate) stop_held: Option<std::sync::mpsc::Sender<()>>,
}

impl Seams {
    /// The scan, and start-up's wait for Scratch with its test seam.
    pub(crate) fn split(self) -> (Scan, ScratchWait) {
        let wait = ScratchWait {
            #[cfg(test)]
            stop_held: self.stop_held,
            ..ScratchWait::default()
        };
        (self.scan, wait)
    }
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            scan: Arc::new(free_folder),
            #[cfg(test)]
            stop_held: None,
        }
    }
}

/// The first folder of `<base>/Scratch`, `Scratch 2`, ... that does not exist.
pub(crate) fn free_folder(base: &Path) -> Option<PathBuf> {
    (1_u32..=u32::MAX)
        .map(|n| match n {
            1 => base.join(SCRATCH),
            _ => base.join(format!("{SCRATCH} {n}")),
        })
        .find(|folder| !folder.exists())
}

/// What reaches the actor about the Scratch matter.
pub(crate) enum ScratchInput {
    /// The scan's answer: the free folder, none, or `off_actor`'s failure (no answer in time).
    Scanned(Result<Option<PathBuf>, Error>),
    /// [`READY_WAIT`] passed since the scan began.
    Waited,
}

/// Where the Scratch matter stands for start-up.
#[derive(Default)]
pub(crate) struct ScratchWait {
    /// The scan runs off the actor.
    scanning: bool,
    /// Ready no longer waits for Scratch: [`READY_WAIT`] passed or ready went out.
    waited: bool,
    /// The [`READY_WAIT`] timer.
    timer: Option<AbortHandle>,
    /// Tests: shutdown waits for the scan too, and says so here each time it does, so a test
    /// can answer the scan while the host stops (`HostConfig::with_scratch_stop_held`).
    #[cfg(test)]
    pub(crate) stop_held: Option<std::sync::mpsc::Sender<()>>,
}

/// Starts the scan for the Scratch matter's folder, and the [`READY_WAIT`] timer. With no
/// default matters folder (no Documents folder, issue #365) there is no Scratch matter this run;
/// while Documents is still being looked up the scan waits for its answer ([`start_late`]).
pub(super) fn start(actor: &mut Actor) {
    let Some(base) = super::documents::default_matters_dir(actor).map(Path::to_path_buf) else {
        if super::documents::pending(actor) {
            tracing::info!("the Scratch matter waits for the Documents lookup");
        } else {
            tracing::warn!("no default matters folder: no Scratch matter");
        }
        return;
    };
    actor.scratch_wait.scanning = true;
    let timer = actor.post_after(READY_WAIT, Input::Scratch(ScratchInput::Waited));
    actor.scratch_wait.timer = Some(timer);
    let (clock, scan) = (Arc::clone(&actor.clock), Arc::clone(&actor.scratch_scan));
    actor.spawn_post(async move {
        let found = off_actor(clock, SCAN_JOB, move || Ok(scan(&base))).await;
        Input::Scratch(ScratchInput::Scanned(found))
    });
}

/// The Documents lookup answered after the start (`documents.rs`): the scan starts as at the
/// start when the host has no Scratch matter, is not finding or making one, and is not
/// stopping. Started during start-up, ready waits for it as it does at the start; once serving,
/// nothing waits, and a Scratch made later joins the list as any new matter does.
pub(super) fn start_late(actor: &mut Actor) {
    let listed = actor
        .matters
        .values()
        .any(|matter| matter.row.kind == MatterKind::Scratch);
    let stopping = matches!(actor.phase, HostPhase::Stopping(_));
    if listed || stopping || holds_start(actor) {
        return;
    }
    start(actor);
}

/// One [`ScratchInput`].
pub(super) fn on_input(actor: &mut Actor, input: ScratchInput) {
    match input {
        ScratchInput::Scanned(found) => scanned(actor, found),
        ScratchInput::Waited => {
            if !actor.scratch_wait.waited && holds_start(actor) {
                tracing::warn!(
                    waited_ms = READY_WAIT.as_millis(),
                    "the Scratch matter is not made yet; start-up goes on without it"
                );
            }
            actor.scratch_wait.waited = true;
            actor.scratch_wait.timer = None;
        }
    }
}

/// The scan answered: the create starts on the new matter's worker when Scratch is still wanted
/// (no Scratch matter listed or being made, the host not stopping, this scan not answered yet).
fn scanned(actor: &mut Actor, found: Result<Option<PathBuf>, Error>) {
    if !std::mem::replace(&mut actor.scratch_wait.scanning, false) {
        tracing::warn!("a Scratch folder scan answered twice");
        return;
    }
    if matches!(actor.phase, HostPhase::Stopping(_)) {
        tracing::info!("the host is stopping: no Scratch matter");
        return;
    }
    let listed = actor
        .matters
        .values()
        .any(|matter| matter.row.kind == MatterKind::Scratch);
    if listed || actor.scratch.is_some() {
        return;
    }
    let folder = match found {
        Ok(Some(folder)) => folder,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "no Scratch matter: its folder scan failed");
            return;
        }
    };
    let begun = super::matters::begin_matter(
        actor,
        (SCRATCH.to_owned(), MatterKind::Scratch, (None, None)),
        folder,
        None,
        MatterTag::Scratch,
    );
    match begun {
        Ok(pending) => actor.scratch = Some(pending),
        Err(error) => tracing::warn!(error = %Cause(&error), "could not start the Scratch matter"),
    }
}

/// Whether the Scratch matter is still being found or made.
fn holds_start(actor: &Actor) -> bool {
    actor.scratch_wait.scanning || actor.scratch.is_some()
}

/// Whether the Scratch matter holds the phase where it is: start-up until it is made or
/// [`READY_WAIT`] passed; shutdown while its create runs on its worker (as a `CreateMatter`'s
/// does), never for the scan.
pub(super) fn holds(actor: &Actor) -> bool {
    match actor.phase {
        HostPhase::Starting(_) => !actor.scratch_wait.waited && holds_start(actor),
        HostPhase::Stopping(_) => actor.scratch.is_some() || stop_held(actor),
        HostPhase::Serving => false,
    }
}

/// Tests: whether shutdown waits for a scan still running (`ScratchWait::stop_held`).
#[cfg(test)]
fn stop_held(actor: &Actor) -> bool {
    let wait = &actor.scratch_wait;
    match &wait.stop_held {
        Some(told) if wait.scanning => {
            let _gone = told.send(());
            true
        }
        _ => false,
    }
}

/// Shutdown never waits for the scan.
#[cfg(not(test))]
const fn stop_held(_: &Actor) -> bool {
    false
}

/// Ready went out: the [`READY_WAIT`] timer is disarmed.
pub(super) fn serving(actor: &mut Actor) {
    actor.scratch_wait.waited = true;
    if let Some(timer) = actor.scratch_wait.timer.take() {
        timer.abort();
    }
}

#[cfg(test)]
mod tests;
