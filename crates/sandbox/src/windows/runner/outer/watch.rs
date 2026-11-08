//! The git watch in the outer runner (`git_watch.rs`): started on the matter's folder before
//! the inner runner starts, so it is armed before the agent runs, and stopped once the job is
//! ended at the session's end. Its thread runs as the outer does, as the lawyer.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use clauseo_protocol::MatterId;

use super::super::end::EndSignal;
use super::super::git_watch::os::{DirWatch, Stop};
use super::super::git_watch::{Guard, Session, watch};
use super::super::{LaunchMessage, RunExit};
use super::{End, Failure, os, say};
use crate::windows::set_aside::{Failed, GitGuard, Live};

/// How long the outer waits at its end for the watch to stop. A rescan or a retry in flight
/// finishes first; past this the process exits without it, and a move cut short is settled at
/// the manifest's next load, as a crash's is.
const STOP_WAIT: Duration = Duration::from_secs(1);

/// The watch's thread, running.
pub(in super::super) struct Running {
    stop: Arc<Stop>,
    done: mpsc::Receiver<()>,
}

impl Running {
    /// Stops the watch and waits up to [`STOP_WAIT`] for its thread to end.
    pub(in super::super) fn stop(self) {
        self.stop.set();
        if self.done.recv_timeout(STOP_WAIT).is_err() {
            say(
                "clauseo-sandbox-run: git watch: still busy at the session's end; the check \
                 after the session covers it",
            );
        }
    }
}

/// Starts the watch on `root`, `matter`'s folder, and returns once it is armed or, when it
/// can't be, has said so and gone over to rescans. Only a thread that can't be started fails.
fn start(root: &Path, data_dir: &Path, matter: MatterId, end: &End) -> io::Result<Running> {
    let stop = Arc::new(Stop::new()?);
    let (armed, first_armed) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let (root, data_dir, end, thread_stop) = (
        root.to_owned(),
        data_dir.to_owned(),
        Arc::clone(end),
        Arc::clone(&stop),
    );
    thread::Builder::new()
        .name("git-watch".to_owned())
        .spawn(move || {
            let guard = OsGuard(GitGuard::new(&root, &data_dir, &matter));
            let session = OsSession {
                end,
                stop: &thread_stop,
            };
            let first = DirWatch::arm(&root, &thread_stop);
            let _sent = armed.send(());
            watch(
                first,
                || DirWatch::arm(&root, &thread_stop),
                &guard,
                &session,
            );
            let _sent = finished.send(());
        })?;
    // The thread said it armed (or couldn't); a thread that ended first ends this wait too.
    first_armed.recv().unwrap_or(());
    Ok(Running { stop, done })
}

/// The set-aside's live check, at the time of each call.
struct OsGuard(GitGuard);

impl Guard for OsGuard {
    fn changed(&self, paths: &[PathBuf]) -> Live {
        self.0.changed(paths, now())
    }

    fn rescan(&self) -> Live {
        self.0.rescan(now())
    }

    fn retry(&self, failed: &[Failed]) -> Live {
        self.0.retry(failed, now())
    }
}

/// The session as the watch sees it: the outer's end path, its stderr and the stop.
struct OsSession<'a> {
    end: End,
    stop: &'a Stop,
}

impl Session for OsSession<'_> {
    fn end(&self) {
        // Every end path's own: the job ended and emptied, then the sweep task requested.
        self.end.trigger(EndSignal::Failed);
    }

    fn say(&self, line: &str) {
        say(line);
    }

    fn pause(&self, wait: Duration) -> bool {
        self.stop.wait(wait)
    }

    fn settle(&self, wait: Duration) {
        pause(wait);
    }
}

/// The time a set-aside's pass folder is named for.
#[expect(
    clippy::disallowed_methods,
    reason = "the watch's moves are named for the time they happen; the tests pass a time"
)]
fn now() -> SystemTime {
    SystemTime::now()
}

/// The pause between two tries at a `.git` once the session ended.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded pause while the ended agent's handles close; the tests inject it \
              (`Session::settle`)"
)]
fn pause(wait: Duration) {
    thread::sleep(wait);
}

/// Starts the watch on `message`'s matter folder and keeps it in `end`'s steps, for their
/// [`stop_watch`](super::end::OsEnd::stop_watch). Only a thread that can't be started fails
/// the launch.
pub(super) fn keep(message: &LaunchMessage, end: &End) -> Result<(), Failure> {
    let root = Path::new(&message.matter_root);
    let running = start(root, &end.steps().data_dir, message.matter, end)
        .map_err(|error| Failure::new(RunExit::Spawn, os("the git watch's thread", &error)))?;
    end.steps().keep_watch(running);
    Ok(())
}
