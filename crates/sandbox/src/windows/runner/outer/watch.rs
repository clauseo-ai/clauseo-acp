//! The git watch in the outer runner (`git_watch.rs`): started on the matter's folder, and on its
//! notes folder when the run can write one, before the inner runner starts, so it is armed
//! before the agent runs, and stopped once the job is ended and empty at the session's end,
//! after one last rescan. Its threads run as the outer does, as the lawyer. A watch that can't be
//! armed refuses the launch, plainly. A thread that panics or ends before it is stopped ends the
//! session: fail closed. Each watch has a watchdog thread (`git_watch::watchdog`) that ends the
//! session when one check stays busy past the budget: a call blocked on an agent's oplock never
//! leaves the matter unwatched while the agent runs. The watches' records (the lawyer's notices
//! and the set-asides) go to the host's denial pipe (`denial_pipe`), connected just before
//! (`start.rs`), or only to the log when the launch names none. The agent's state and temp
//! folders get watches that only note which file IDs left them, so a folder moved from there
//! into the matter counts gone by its check whatever its creation time says (`git_watch::pairs`);
//! one that can't be armed is passed over.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use clauseo_protocol::MatterId;

use super::super::end::EndSignal;
use super::super::git_watch::os::{DirWatch, Stop};
use super::super::git_watch::{
    Cause, Changes, Dog, Guard, Paired, Pairs, Pulse, Session, feed, watch, watchdog,
};
use super::super::{LaunchMessage, RunExit};
use super::{End, Failure, os, say};
use crate::windows::denial_pipe::Record;
use crate::windows::denial_pipe::sink::RecordSink;
use crate::windows::set_aside::{Failed, GitGuard, Hooks, Live};

/// How long the outer waits at its end for each watch's last rescan and stop. Past this the
/// process exits without it, and a move cut short is settled at the manifest's next load, as a
/// crash's is; the check after the session covers the rest.
const STOP_WAIT: Duration = Duration::from_secs(10);
/// How long the outer then waits for the records still queued to reach the denial pipe.
const RECORDS_WAIT: Duration = Duration::from_secs(2);

/// The watches' threads, running.
pub(in super::super) struct Running {
    stop: Arc<Stop>,
    done: Vec<mpsc::Receiver<()>>,
    records: Arc<RecordSink>,
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
        self.records.drain(RECORDS_WAIT);
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
    (stop, records, pairs): (&Arc<Stop>, &Arc<RecordSink>, &Arc<Pairs>),
) -> Result<mpsc::Receiver<()>, NotStarted> {
    let pairs = Arc::clone(pairs);
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
    let (records, dog_records) = (Arc::clone(records), Arc::clone(records));
    thread::Builder::new()
        .name("git-watch".to_owned())
        .spawn(move || {
            let mut watching = false;
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let guard = OsGuard(match folder {
                    Folder::Matter => GitGuard::new(&root, &data_dir, &matter),
                    Folder::Other => GitGuard::notes(&root, &data_dir, &matter),
                });
                let session = OsSession {
                    end: Arc::clone(&end),
                    stop: &stop,
                    pulse: &pulse,
                    records: &records,
                    paired: Paired {
                        pairs,
                        root: guard.0.root().to_owned(),
                    },
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
                records.send(&Record::notice(Cause::WatchFailed.notice()));
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
        records: dog_records,
    };
    thread::Builder::new()
        .name("git-watchdog".to_owned())
        .spawn(move || watchdog(&dog_pulse, &dog))
        .map_err(NotStarted::Thread)?;
    Ok(done)
}

/// Starts the watches that only feed the pairing (`git_watch::feed`): on the agent's state
/// folder, and on its temp folder when that isn't inside the state folder. A folder that can't
/// be watched (or a thread that can't start) is logged and passed over, never refusing the
/// launch: the pairing only adds to the creation-time rule, and the matter stays watched.
fn pairing(
    message: &LaunchMessage,
    stop: &Arc<Stop>,
    pairs: &Arc<Pairs>,
) -> Vec<mpsc::Receiver<()>> {
    let state = PathBuf::from(&message.state);
    let mut roots = vec![state.clone()];
    roots.extend(
        message
            .tmpdir
            .as_ref()
            .map(PathBuf::from)
            .filter(|tmp| !tmp.starts_with(&state)),
    );
    let mut done = Vec::new();
    for root in roots {
        let (stop, pairs) = (Arc::clone(stop), Arc::clone(pairs));
        let (finished, ended) = mpsc::channel();
        let started = thread::Builder::new()
            .name("git-pairing".to_owned())
            .spawn(move || {
                let ran = catch_unwind(AssertUnwindSafe(|| match DirWatch::arm(&root, &stop) {
                    Ok(source) => feed(source, &Paired { pairs, root }, monotonic),
                    Err(error) => tracing::warn!(
                        code = error.raw_os_error(),
                        "a pairing watch couldn't be armed; passed over"
                    ),
                }));
                if ran.is_err() {
                    tracing::warn!("a pairing watch panicked; it notes no more");
                }
                let _sent = finished.send(());
            });
        match started {
            Ok(_) => done.push(ended),
            Err(error) => tracing::warn!(%error, "a pairing watch's thread couldn't start"),
        }
    }
    done
}

/// The watchdog's side of the session: the outer's end path, the denial pipe and the stop.
struct OsDog {
    end: End,
    stop: Arc<Stop>,
    records: Arc<RecordSink>,
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
            self.records.send(&Record::notice(Cause::Stalled.notice()));
        }
        // As a panic's: the job ended and emptied, then the end sweeper started.
        self.end.trigger(EndSignal::Failed);
    }
}

/// The set-aside's live check, at the time of each call.
struct OsGuard(GitGuard);

impl Guard for OsGuard {
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live {
        let fresh = changes.fresh_paths();
        let mut live = self.0.changed(
            &changes.added,
            (&changes.moved_folders, &fresh),
            now(),
            hooks,
        );
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

/// The session as the watch sees it: the outer's end path, its stderr, the denial pipe, the
/// stop and what it shares with its watchdog.
struct OsSession<'a> {
    end: End,
    stop: &'a Stop,
    pulse: &'a Pulse,
    records: &'a RecordSink,
    paired: Paired,
}

impl Session for OsSession<'_> {
    fn end(&self) -> bool {
        // The job ended, and waited on until it is empty: only then may the moves go ahead. The
        // end sweeper's logon starts after, off the moves' path.
        self.end.kill_now(EndSignal::Failed)
    }

    fn say(&self, line: &str) {
        say(line);
    }

    fn record(&self, record: &Record) {
        self.records.send(record);
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

    fn paired(&self) -> Option<&Paired> {
        Some(&self.paired)
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
/// in `end`'s steps, for their [`stop_watch`](super::end::OsSweeper::stop_watch). A folder that
/// can't be watched refuses the launch, in plain words; so does a thread that can't be started.
pub(super) fn keep(
    message: &LaunchMessage,
    end: &End,
    records: Arc<RecordSink>,
) -> Result<(), Failure> {
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
    let pairs = Arc::new(Pairs::default());
    let mut done = pairing(message, &stop, &pairs);
    for (root, folder) in &folders {
        let started = start(
            (root, *folder),
            &end.steps().data_dir,
            message.matter,
            end,
            (&stop, &records, &pairs),
        );
        match started {
            Ok(finished) => done.push(finished),
            Err(not) => {
                // The watches already running stop with the failed launch.
                end.steps().keep_watch(Running {
                    stop,
                    done,
                    records,
                });
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
    end.steps().keep_watch(Running {
        stop,
        done,
        records,
    });
    Ok(())
}
