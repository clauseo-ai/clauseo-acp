//! The git watch in the outer runner (`git_watch.rs`): started on the matter's folder, and on its
//! notes folder when the run can write one, before the inner runner starts, so it is armed
//! before the agent runs, and stopped once the job is ended and empty at the session's end,
//! after one last rescan. Its threads run as the outer does, as the lawyer. A thread that panics
//! or ends before it is stopped ends the session: fail closed.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use clauseo_protocol::MatterId;

use super::super::end::EndSignal;
use super::super::git_watch::os::{DirWatch, Stop};
use super::super::git_watch::{Cause, Changes, Guard, Session, watch};
use super::super::{LaunchMessage, RunExit};
use super::{End, Failure, os, say};
use crate::windows::set_aside::{Failed, GitGuard, Hooks, Live};

/// How long the outer waits at its end for each watch's last rescan and stop. Past this the
/// process exits without it, and a move cut short is settled at the manifest's next load, as a
/// crash's is; the check after the session covers the rest.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// The watches' threads, running.
pub(in super::super) struct Running {
    stop: Arc<Stop>,
    done: Vec<mpsc::Receiver<()>>,
}

impl Running {
    /// Stops the watches and waits up to [`STOP_WAIT`] for their threads to end.
    pub(in super::super) fn stop(self) {
        self.stop.set();
        for done in &self.done {
            if done.recv_timeout(STOP_WAIT).is_err() {
                say(
                    "clauseo-sandbox-run: git watch: still busy at the session's end; the check \
                     after the session covers it",
                );
            }
        }
    }
}

/// Which folder a watch covers, and so what it knows at its start.
#[derive(Clone, Copy)]
enum Folder {
    /// The matter's: the set-aside's record says what was there at launch.
    Matter,
    /// Another folder the agent can write (the notes folder): a scan says.
    Other,
}

/// Starts a watch on `root` for `matter` and returns once it is armed or, when it can't be,
/// has said so and gone over to rescans. Only a thread that can't be started fails.
fn start(
    (root, folder): (&Path, Folder),
    data_dir: &Path,
    matter: MatterId,
    end: &End,
    stop: &Arc<Stop>,
) -> io::Result<mpsc::Receiver<()>> {
    let (armed, first_armed) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let (root, data_dir, end, stop) = (
        root.to_owned(),
        data_dir.to_owned(),
        Arc::clone(end),
        Arc::clone(stop),
    );
    thread::Builder::new()
        .name("git-watch".to_owned())
        .spawn(move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let guard = OsGuard(match folder {
                    Folder::Matter => GitGuard::new(&root, &data_dir, &matter),
                    Folder::Other => GitGuard::scanned(&root, &data_dir, &matter),
                });
                let session = OsSession {
                    end: Arc::clone(&end),
                    stop: &stop,
                };
                let first = DirWatch::arm(guard.0.root(), &stop);
                let _sent = armed.send(());
                watch(first, || DirWatch::arm(&root, &stop), &guard, &session);
            }));
            // A watch ends only once stopped; one that panicked, or ended first, leaves the
            // folder unwatched while the agent may still run.
            let stopped = stop.wait(Duration::ZERO);
            if ran.is_err() || !stopped {
                tracing::warn!(
                    panicked = ran.is_err(),
                    "the git watch ended before its stop"
                );
                say(&format!(
                    "clauseo-sandbox-run: {}",
                    Cause::WatchFailed.notice()
                ));
                end.trigger(EndSignal::Failed);
            }
            let _sent = finished.send(());
        })?;
    // The thread said it armed (or couldn't); a thread that ended first ends this wait too.
    first_armed.recv().unwrap_or(());
    Ok(done)
}

/// The set-aside's live check, at the time of each call.
struct OsGuard(GitGuard);

impl Guard for OsGuard {
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live {
        let mut live = self.0.changed(&changes.added, now(), hooks);
        let relinked = self.0.relinked(&changes.relinked, now(), hooks);
        live.moved.extend(relinked.moved);
        live.failed.extend(relinked.failed);
        live.gone.extend(relinked.gone);
        live
    }

    fn rescan(&self, hooks: &impl Hooks) -> Live {
        self.0.rescan(now(), hooks)
    }

    fn retry(&self, failed: &[Failed], hooks: &impl Hooks) -> Live {
        self.0.retry(failed, now(), hooks)
    }
}

/// The session as the watch sees it: the outer's end path, its stderr and the stop.
struct OsSession<'a> {
    end: End,
    stop: &'a Stop,
}

impl Session for OsSession<'_> {
    fn end(&self) {
        // Every end path's own: the job ended and emptied, then the end sweeper started.
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

    fn now(&self) -> Duration {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        let origin = *ORIGIN.get_or_init(clock);
        clock().saturating_duration_since(origin)
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

/// The monotonic clock, for the scans' budget.
#[expect(
    clippy::disallowed_methods,
    reason = "the watch ends a session whose scan runs past its budget; the tests inject the \
              clock (`Session::now`)"
)]
fn clock() -> Instant {
    Instant::now()
}

/// The pause between two tries at an item once the session ended.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded pause while the ended agent's handles close; the tests inject it \
              (`Session::settle`)"
)]
fn pause(wait: Duration) {
    thread::sleep(wait);
}

/// Starts the watches on `message`'s matter folder and its notes folder, if any, and keeps them
/// in `end`'s steps, for their [`stop_watch`](super::end::OsSweeper::stop_watch). Only a thread
/// that can't be started fails the launch.
pub(super) fn keep(message: &LaunchMessage, end: &End) -> Result<(), Failure> {
    let stop = Arc::new(
        Stop::new().map_err(|error| Failure::new(RunExit::Spawn, os("the git watch", &error)))?,
    );
    let mut folders = vec![(PathBuf::from(&message.matter_root), Folder::Matter)];
    folders.extend(
        message
            .notes_root
            .as_ref()
            .map(|notes| (PathBuf::from(notes), Folder::Other)),
    );
    let mut done = Vec::new();
    for (root, folder) in &folders {
        let started = start(
            (root, *folder),
            &end.steps().data_dir,
            message.matter,
            end,
            &stop,
        );
        match started {
            Ok(finished) => done.push(finished),
            Err(error) => {
                // The watches already running stop with the failed launch.
                end.steps().keep_watch(Running { stop, done });
                return Err(Failure::new(
                    RunExit::Spawn,
                    os("the git watch's thread", &error),
                ));
            }
        }
    }
    end.steps().keep_watch(Running { stop, done });
    Ok(())
}
