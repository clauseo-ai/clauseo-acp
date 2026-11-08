//! The git watch in the outer runner (`git_watch.rs`): started on the matter's folder, and on its
//! notes folder when the run can write one, before the inner runner starts, so it is armed
//! before the agent runs, and stopped once the job is ended and empty at the session's end,
//! after one last rescan. Its threads run as the outer does, as the lawyer. A watch that can't be
//! armed refuses the launch, plainly. A thread that panics or ends before it is stopped ends the
//! session: fail closed. Each watch has a watchdog thread (`git_watch::watchdog`) that ends the
//! session when one check stays busy past the budget: a call blocked on an agent's oplock never
//! leaves the matter unwatched while the agent runs.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use clauseo_protocol::MatterId;

use super::super::end::EndSignal;
use super::super::git_watch::os::{DirWatch, Stop};
use super::super::git_watch::{Cause, Changes, Dog, Guard, Pulse, Session, watch, watchdog};
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

/// Why a watch didn't start.
enum NotStarted {
    /// Its thread couldn't be started.
    Thread(io::Error),
    /// The folder couldn't be watched.
    Arm(io::Error),
}

/// Starts a watch on `root` for `matter`, and its watchdog, and returns once it is armed.
fn start(
    (root, folder): (&Path, Folder),
    data_dir: &Path,
    matter: MatterId,
    end: &End,
    stop: &Arc<Stop>,
) -> Result<mpsc::Receiver<()>, NotStarted> {
    let (armed, first_armed) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let pulse = Arc::new(Pulse::default());
    let (root, data_dir, end, stop) = (
        root.to_owned(),
        data_dir.to_owned(),
        Arc::clone(end),
        Arc::clone(stop),
    );
    let (dog_end, dog_stop, dog_pulse) = (Arc::clone(&end), Arc::clone(&stop), Arc::clone(&pulse));
    thread::Builder::new()
        .name("git-watch".to_owned())
        .spawn(move || {
            let mut watching = false;
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let guard = OsGuard(match folder {
                    Folder::Matter => GitGuard::new(&root, &data_dir, &matter),
                    Folder::Other => GitGuard::scanned(&root, &data_dir, &matter),
                });
                let session = OsSession {
                    end: Arc::clone(&end),
                    stop: &stop,
                    pulse: &pulse,
                };
                match DirWatch::arm(guard.0.root(), &stop) {
                    Ok(source) => {
                        watching = true;
                        let _sent = armed.send(Ok(()));
                        watch(source, &guard, &session);
                    }
                    Err(error) => {
                        let _sent = armed.send(Err(error));
                    }
                }
            }));
            // A watch ends only once stopped; one that panicked, or ended first, leaves the
            // folder unwatched while the agent may still run. One never armed refused the launch.
            let stopped = stop.wait(Duration::ZERO);
            if ran.is_err() || (watching && !stopped) {
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
        })
        .map_err(NotStarted::Thread)?;
    // The thread said it armed (or couldn't); a thread that ended first ends this wait too.
    match first_armed.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(NotStarted::Arm(error)),
        Err(_) => {
            return Err(NotStarted::Arm(io::Error::other(
                "the watch's thread ended",
            )));
        }
    }
    let dog = OsDog {
        end: dog_end,
        stop: dog_stop,
    };
    thread::Builder::new()
        .name("git-watchdog".to_owned())
        .spawn(move || watchdog(&dog_pulse, &dog))
        .map_err(NotStarted::Thread)?;
    Ok(done)
}

/// The watchdog's side of the session: the outer's end path, its stderr and the stop.
struct OsDog {
    end: End,
    stop: Arc<Stop>,
}

impl Dog for OsDog {
    fn wait(&self, wait: Duration) -> bool {
        self.stop.wait(wait)
    }

    fn now(&self) -> Duration {
        monotonic()
    }

    fn stall(&self, first: bool) {
        if first {
            say(&format!("clauseo-sandbox-run: {}", Cause::Stalled.notice()));
        }
        // As a panic's: the job ended and emptied, then the sweep task requested.
        self.end.trigger(EndSignal::Failed);
    }
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

/// The session as the watch sees it: the outer's end path, its stderr, the stop and what it
/// shares with its watchdog.
struct OsSession<'a> {
    end: End,
    stop: &'a Stop,
    pulse: &'a Pulse,
}

impl Session for OsSession<'_> {
    fn end(&self) -> bool {
        // The job ended, and waited on until it is empty: only then may the moves go ahead. The
        // sweep task's request follows, off the moves' path.
        self.end.kill_now(EndSignal::Failed)
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
        monotonic()
    }

    fn pulse(&self) -> &Pulse {
        self.pulse
    }
}

/// A monotonic reading since the first, shared by the watches and their watchdogs.
fn monotonic() -> Duration {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let origin = *ORIGIN.get_or_init(clock);
    clock().saturating_duration_since(origin)
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
/// in `end`'s steps, for their [`stop_watch`](super::end::OsEnd::stop_watch). A folder that
/// can't be watched refuses the launch, in plain words; so does a thread that can't be started.
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
            Err(not) => {
                // The watches already running stop with the failed launch.
                end.steps().keep_watch(Running { stop, done });
                return Err(Failure::new(
                    RunExit::Spawn,
                    match not {
                        NotStarted::Thread(error) => os("the git watch's thread", &error),
                        NotStarted::Arm(error) => format!(
                            "Clauseo can't watch the matter's folder for changes, so it didn't \
                             start the session ({})",
                            os("the git watch", &error)
                        ),
                    },
                ));
            }
        }
    }
    end.steps().keep_watch(Running { stop, done });
    Ok(())
}
