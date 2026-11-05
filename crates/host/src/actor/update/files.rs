//! The updater's file thread, `clauseo-update`: every touch of `update.json` and of last run's
//! downloads runs here, one at a time and in the order the actor asked, so the actor never
//! waits on the disk and a clear at start-up never lands after the marker an install writes.
//! Each job posts [`UpdateInput::Files`] back when done; the actor counts what is still out, so
//! an install's exit waits for its marker.

use std::sync::mpsc;
use std::thread;

use clauseo_settings::Locations;

use super::UpdateInput;
use crate::actor::input::Input;
use crate::update::{UpdateMarker, clear_marker, find_cached, read_update_marker, write_marker};

/// The thread's name.
pub(super) const THREAD_NAME: &str = "clauseo-update";

/// One job for the file thread.
#[derive(Debug)]
pub(super) enum Job {
    /// Start-up's read: `update.json` and last run's download, given the running version.
    Read { current: String },
    /// Write `update.json`.
    Write(UpdateMarker),
    /// Delete `update.json`.
    Clear,
}

/// What a job found, posted back to the actor.
#[derive(Debug)]
pub(crate) enum Done {
    /// The marker and last run's download.
    Read {
        marker: Option<UpdateMarker>,
        cached: Option<crate::update::CachedDownload>,
    },
    /// The marker's write ended: `true` when it is on disk (a failure is already logged).
    Wrote(bool),
    /// The marker's delete ended; a failure is already logged.
    Cleared,
}

/// The way to the file thread. Dropping it ends the thread once its queue is empty.
#[derive(Debug)]
pub(super) struct Files {
    jobs: Option<mpsc::Sender<Job>>,
    post: async_channel::Sender<Input>,
}

impl Files {
    /// Starts the thread over `locations`, posting each job's end to `post`.
    pub(super) fn start(locations: Locations, post: async_channel::Sender<Input>) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let posts = post.clone();
        let started = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || serve(&locations, &queue, &posts));
        let jobs = match started {
            Ok(_detached) => Some(jobs),
            Err(error) => {
                tracing::error!(kind = %error.kind(), "could not start {THREAD_NAME}");
                None
            }
        };
        Self { jobs, post }
    }

    /// Queues `job`; its end is always posted. With no thread, it is answered at once as if
    /// it found nothing (no marker, no download) or wrote nothing, never run on the actor.
    pub(super) fn send(&self, job: Job) {
        let job = match &self.jobs {
            Some(jobs) => match jobs.send(job) {
                Ok(()) => return,
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        tracing::warn!("{THREAD_NAME} is gone; an update file job is skipped");
        let done = match job {
            Job::Read { .. } => Done::Read {
                marker: None,
                cached: None,
            },
            Job::Write(_) => Done::Wrote(false),
            Job::Clear => Done::Cleared,
        };
        let _gone = self.post.try_send(Input::Update(UpdateInput::Files(done)));
    }
}

/// The thread's loop: each job in turn until the actor drops its sender.
fn serve(locations: &Locations, queue: &mpsc::Receiver<Job>, post: &async_channel::Sender<Input>) {
    for job in queue {
        let done = run(locations, job);
        // The actor may be gone by then; nobody is left to tell.
        let _gone = post.try_send(Input::Update(UpdateInput::Files(done)));
    }
}

/// Runs one job. Never on the host thread: the assertion holds every test that writes a
/// marker to it.
pub(super) fn run(locations: &Locations, job: Job) -> Done {
    debug_assert_ne!(
        thread::current().name(),
        Some(crate::actor::THREAD_NAME),
        "update.json is touched only off the actor"
    );
    match job {
        Job::Read { current } => Done::Read {
            marker: read_update_marker(locations),
            cached: find_cached(locations, &current),
        },
        Job::Write(marker) => match write_marker(locations, &marker) {
            Ok(()) => Done::Wrote(true),
            Err(error) => {
                // No install then: a failed one would report nothing at the next start.
                tracing::warn!(kind = %error.kind(), "update.json was not written");
                Done::Wrote(false)
            }
        },
        Job::Clear => {
            if let Err(error) = clear_marker(locations) {
                tracing::warn!(kind = %error.kind(), "update.json was not deleted");
            }
            Done::Cleared
        }
    }
}
