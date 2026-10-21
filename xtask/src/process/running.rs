//! A started command and the wait for it: one thread reads each output pipe, one waits for the
//! exit, and the caller's thread takes their events from one channel. The phases of the wait are
//! timed with `clock::timed` around each receive, so no timer thread and no sleep is needed.
//!
//! The run ends when the command has exited, its tree is empty and its pipes are closed. When the
//! command exits, and at its limit, its tree is asked to stop (`Tree::terminate`: SIGTERM to the
//! group on Unix, the whole stop on Windows) and has the grace (`KILL_GRACE` unless its `Cmd` sets
//! another) to empty, checked every `POLL` once the command has exited, as
//! `ui/app/scripts/buildtime-proc.mjs` does; what is still in it then is killed (`Tree::kill`:
//! SIGKILL on Unix). So a helper a tool forked into its group gets SIGTERM and the grace after
//! every step, not SIGKILL. A process that left the tree can hold a pipe for ever, so once the
//! tree is empty, or after the kill, the run waits at most `DRAIN` for the pipes, and a run that
//! exited says when one was still open then. When the run ends, or its `Running` is dropped, what
//! is left of the tree is killed (`Tree::finish`); a drop before the end stops it the same way as
//! a timeout first.

use super::{Cmd, spawn};
use crate::clock::timed;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use super::unix::Group as OsTree;
#[cfg(windows)]
use super::windows::Job as OsTree;

/// How long a tree has between SIGTERM and SIGKILL, as in `ui/app/scripts/buildtime-proc.mjs`,
/// unless its `Cmd` sets another (`Cmd::kill_grace`). On Windows the stop at the limit already
/// ends the job, so this is only how long its pipes have.
pub(super) const KILL_GRACE: Duration = Duration::from_secs(5);
/// How long a run waits for the output pipes once its tree is empty or was killed.
const DRAIN: Duration = Duration::from_secs(1);
/// How often a tree that was asked to stop is checked for being empty, by a run once its command
/// has exited and by the leader. Nothing says when a group empties, so it is asked.
pub(super) const POLL: Duration = Duration::from_millis(50);
/// Size of each read from a running command's output pipe.
const CHUNK: usize = 8192; // 8 KiB

/// What a command's tree is, written down once: `unix.rs` (a process group) and `windows.rs` (a
/// Job Object) both implement it, and this file uses nothing else of them, so a change to the
/// contract fails the build of both sides, not only the one a developer builds.
pub(super) trait Tree: Sized {
    /// What a tree is called in errors.
    const NAME: &'static str;

    /// Makes `command`, whose output goes where `streams` says, start in a new tree.
    fn prepare(command: &mut Command, streams: Streams);

    /// The tree of `child`, just started from a `prepare`d command. When that fails, what was
    /// started is ended and the error returned; the caller reaps.
    fn adopt(child: &mut Child) -> io::Result<Self>;

    /// Asks every process in the tree to stop.
    fn terminate(&self) -> io::Result<()>;

    /// Ends every process in the tree.
    fn kill(&self) -> io::Result<()>;

    /// Whether no process is left in the tree. Asked only after `terminate`.
    fn empty(&self) -> bool;

    /// Ends what is left of the tree once its run is over, and lets it go.
    fn finish(&self);
}

/// Held while the pipes of a command are made and it starts. On macOS std makes a pipe and only
/// then marks its ends close-on-exec (`std/src/sys/pipe/unix.rs`), so a command started by
/// another thread in between would hold them and keep the reader from seeing EOF.
static STARTING: Mutex<()> = Mutex::new(());

/// A pipe made under `STARTING`, as the door makes its own, for a test that hands a command one:
/// made outside it, a command another test thread starts could hold it and delay its EOF.
#[cfg(all(test, unix))]
pub(super) fn pipe() -> io::Result<(io::PipeReader, io::PipeWriter)> {
    let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
    io::pipe()
}

/// How long each phase of a run may last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Limits {
    /// The command's own limit, counted from the start of `Running::finish`.
    pub run: Duration,
    /// From the stop at the exit, the limit or a drop, to the kill.
    pub grace: Duration,
    /// For the pipes to close once the tree is empty or was killed.
    pub drain: Duration,
}

impl Limits {
    /// The limits of a command that may run for `run`: `KILL_GRACE` and `DRAIN` for the rest.
    /// Only tests make the grace and the drain shorter.
    pub(super) fn of(run: Duration) -> Limits {
        Limits {
            run,
            grace: KILL_GRACE,
            drain: DRAIN,
        }
    }

    /// The limits of `cmd`: its own limit and grace (`Cmd::kill_grace`), and `DRAIN`. Every
    /// command xtask runs gets these.
    pub(super) fn of_cmd(cmd: &Cmd) -> Limits {
        Limits {
            grace: cmd.kill_grace,
            ..Limits::of(cmd.timeout)
        }
    }
}

/// Where a command's stdout and stderr go.
#[derive(Debug, Clone, Copy)]
pub(super) enum Streams {
    /// Where the `Command` already sends them: xtask's own, unless a test set them.
    Inherit,
    /// One pipe for both.
    Merged,
    /// A pipe each.
    Separate,
}

/// Which output a chunk came from. Merged output comes as `Out`.
#[derive(Debug, Clone, Copy)]
pub(super) enum Stream {
    Out,
    Err,
}

/// What ended a run other than its command's exit with its tree empty and its pipes closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// It was still running at its limit.
    TimedOut,
    /// A pipe was still open when the drain ran out.
    HeldOpen,
}

/// How a run ended.
#[derive(Debug)]
pub(super) enum Finish {
    Exited(ExitStatus),
    /// It was still running at its limit, so its tree was stopped.
    TimedOut,
}

/// A signal a run sends its tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// `Tree::terminate`: SIGTERM on Unix.
    Terminate,
    /// `Tree::kill`: SIGKILL on Unix.
    Kill,
}

/// What the threads of a run send it. A reader sends nothing at EOF: its thread ends.
enum Event {
    Data(Stream, Vec<u8>),
    /// A pipe could not be read; its reader ends, so it counts as closed.
    ReadFailed(io::Error),
    Exited(io::Result<ExitStatus>),
}

/// Where a run is between its start and its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before its limit, while the command runs: its exit stops what is left of the tree.
    Running,
    /// Asked to stop at the exit, the limit or a drop; has the grace for the command to exit and
    /// the tree to empty.
    Terminating,
    /// The tree emptied within the grace; the pipes have the drain to close.
    Draining,
    /// Killed after the grace; the pipes have the drain to close.
    Killed,
}

/// A started command, until its run ends.
pub(super) struct Running {
    tree: OsTree,
    /// The command as people see it, for the errors.
    shown: String,
    events: Receiver<Event>,
    /// Set once every thread of the run has ended, so `events` gives nothing more.
    closed: bool,
    /// Never sent to while `_ticker` lives: once `events` is closed, the run waits on this for a
    /// poll or the rest of a phase.
    tick: Receiver<()>,
    _ticker: Sender<()>,
    /// Set at the command's exit, whether or not its status could be read.
    exited: bool,
    status: Option<ExitStatus>,
    limits: Limits,
    phase: Phase,
    /// What is left of the current phase.
    left: Duration,
    /// The first thing that cut the run short, if one did.
    cut: Option<Cut>,
    /// The first error of the run: a pipe that could not be read, a failed wait or a failed stop.
    error: Option<io::Error>,
    /// The first stop the OS denied (EPERM) before the command had exited (`Running::stop`).
    denied: Option<io::Error>,
    /// Set once the run has ended and its result was taken, so a drop does not stop it again.
    ended: bool,
}

/// Runs `cmd` in `root` with stdout and stderr as `streams` says, passing each chunk to
/// `on_output`.
pub(super) fn run(
    cmd: &Cmd,
    root: &Path,
    streams: Streams,
    on_output: &mut dyn FnMut(Stream, &[u8]),
) -> io::Result<Finish> {
    let mut command = cmd.program.command();
    command.args(&cmd.args).current_dir(root);
    Running::start(command, &cmd.to_string(), streams, Limits::of_cmd(cmd))?.finish(on_output)
}

impl Running {
    /// Starts `command` with stdin closed, in a tree of its own, with stdout and stderr as
    /// `streams` says, and `limits` for its phases. `shown` names it in errors.
    pub(super) fn start(
        mut command: Command,
        shown: &str,
        streams: Streams,
        limits: Limits,
    ) -> io::Result<Running> {
        command.stdin(Stdio::null());
        let starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        // Started before the command, so registering its group right after its start is one
        // write. Under `STARTING`, like every pipe.
        #[cfg(unix)]
        super::leader::ensure()?;
        OsTree::prepare(&mut command, streams);
        let readers = match streams {
            Streams::Inherit => Vec::new(),
            Streams::Merged => {
                let (reader, writer) = io::pipe()?;
                command.stdout(writer.try_clone()?).stderr(writer);
                vec![(Stream::Out, reader)]
            }
            Streams::Separate => {
                let (out, out_writer) = io::pipe()?;
                let (err, err_writer) = io::pipe()?;
                command.stdout(out_writer).stderr(err_writer);
                vec![(Stream::Out, out), (Stream::Err, err)]
            }
        };
        let mut child = spawn(&mut command)?;
        // The command still holds the write ends of the pipes; drop them so the readers see EOF.
        drop(command);
        let tree = match OsTree::adopt(&mut child) {
            Ok(tree) => tree,
            Err(error) => {
                // `adopt` stopped what it could not adopt; reap it.
                drop(child.wait());
                return Err(error);
            }
        };
        drop(starting);
        let (sender, events) = mpsc::channel();
        // The waiter gets the child only once every thread has started, so a thread that cannot
        // start leaves it here to be stopped and reaped. `hand` dropped unsent ends the waiter.
        let (hand, handed) = mpsc::channel::<Child>();
        let waiter = sender.clone();
        let started = start_thread(move || {
            if let Ok(mut child) = handed.recv() {
                // The receiver is gone once the run has ended, when nobody wants the status.
                drop(waiter.send(Event::Exited(child.wait())));
            }
        })
        .and_then(|()| {
            readers.into_iter().try_for_each(|(stream, reader)| {
                let sender = sender.clone();
                start_thread(move || read_all(stream, reader, &sender))
            })
        });
        // From here only the threads hold a sender, so the channel closes once they have all
        // ended: a run never waits on a sender of its own.
        drop(sender);
        if let Err(error) = started {
            drop(tree.kill());
            drop(child.wait());
            tree.finish();
            return Err(error);
        }
        // Never fails: the waiter holds `handed` until it has received.
        drop(hand.send(child));
        let (ticker, tick) = mpsc::channel();
        Ok(Running {
            tree,
            shown: shown.to_owned(),
            events,
            closed: false,
            tick,
            _ticker: ticker,
            exited: false,
            status: None,
            limits,
            phase: Phase::Running,
            left: limits.run,
            cut: None,
            error: None,
            denied: None,
            ended: false,
        })
    }

    /// Waits for the run to end, passing each chunk to `on_output`, and stops what is left of the
    /// tree. A failed read, wait or stop is the error; else a run past its limit is `TimedOut`;
    /// else a pipe still open after the drain is an error that says so (`held_open`).
    pub(super) fn finish(mut self, on_output: &mut dyn FnMut(Stream, &[u8])) -> io::Result<Finish> {
        self.wait(on_output);
        self.ended = true;
        self.tree.finish();
        // A stop denied before the exit is an error only if the exit never came.
        if let (false, Some(error)) = (self.exited, self.denied.take()) {
            self.keep(self.not_stopped(&error));
        }
        match (self.error.take(), self.cut, self.status) {
            (Some(error), _, _) => Err(error),
            (None, Some(Cut::TimedOut), _) => Ok(Finish::TimedOut),
            (None, Some(Cut::HeldOpen), _) => Err(self.held_open()),
            (None, None, Some(status)) => Ok(Finish::Exited(status)),
            // A run ends before its limit only after the exit, which sets the status or an error.
            (None, None, None) => Err(io::Error::other(format!(
                "the wait for `{}` ended without its exit status",
                self.shown
            ))),
        }
    }

    /// The error of a run whose command exited and whose pipe was still open after the drain. The
    /// tree was empty by then, so what holds the pipe left it.
    fn held_open(&self) -> io::Error {
        io::Error::other(format!(
            "`{}` exited, but its output was still open {} after its {tree} was stopped: a \
             process it started left the {tree} and holds it, and is left running",
            self.shown,
            super::limit_text(self.limits.drain),
            tree = OsTree::NAME,
        ))
    }

    /// Takes events until the command has exited, its tree is empty and its pipes are closed, or
    /// the last phase ran out. Each thread of the run ends after its last event, the waiter after
    /// the exit and each reader at its pipe's EOF, so the channel closes exactly when both are in.
    fn wait(&mut self, on_output: &mut dyn FnMut(Stream, &[u8])) {
        loop {
            if self.phase == Phase::Terminating && self.exited && self.tree.empty() {
                self.enter(Phase::Draining, self.limits.drain);
            }
            if self.closed && matches!(self.phase, Phase::Draining | Phase::Killed) {
                return;
            }
            // Nothing says when the tree empties, so while it has the grace it is asked every
            // `POLL` (above, once the command has exited).
            let wait = if self.phase == Phase::Terminating {
                self.left.min(POLL)
            } else {
                self.left
            };
            let (event, secs) = timed(|| {
                if self.closed {
                    // Never sent to, so this waits the whole `wait`.
                    let ticked = self.tick.recv_timeout(wait);
                    Err(ticked.err().unwrap_or(RecvTimeoutError::Timeout))
                } else {
                    self.events.recv_timeout(wait)
                }
            });
            self.left = self.left.saturating_sub(Duration::from_secs_f64(secs));
            match event {
                Ok(event) => self.take(event, on_output),
                // Every thread has ended, so the exit and every EOF are in.
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
                // A poll, with time left in the phase.
                Err(RecvTimeoutError::Timeout) if !self.left.is_zero() => {}
                Err(RecvTimeoutError::Timeout) => match self.phase {
                    Phase::Running => {
                        self.cut.get_or_insert(Cut::TimedOut);
                        self.terminate();
                    }
                    Phase::Terminating => {
                        self.enter(Phase::Killed, self.limits.drain);
                        self.kill();
                    }
                    Phase::Draining | Phase::Killed => {
                        self.cut.get_or_insert(Cut::HeldOpen);
                        return;
                    }
                },
            }
        }
    }

    /// Takes one event of a thread of the run.
    fn take(&mut self, event: Event, on_output: &mut dyn FnMut(Stream, &[u8])) {
        match event {
            Event::Data(stream, chunk) => on_output(stream, &chunk),
            Event::ReadFailed(error) => self.keep(error),
            Event::Exited(status) => {
                self.exited = true;
                match status {
                    Ok(status) => self.status = Some(status),
                    Err(error) => self.keep(error),
                }
                if self.phase == Phase::Running {
                    // What is left of the tree gets the same stop as at the limit.
                    self.terminate();
                }
            }
        }
    }

    /// Asks the tree to stop (SIGTERM on Unix) and gives it the grace.
    fn terminate(&mut self) {
        self.enter(Phase::Terminating, self.limits.grace);
        self.stop(Stop::Terminate);
    }

    /// Kills the tree (SIGKILL on Unix).
    fn kill(&mut self) {
        self.stop(Stop::Kill);
    }

    /// Sends `stop` to the tree and judges a failure. macOS answers EPERM, not ESRCH, to a signal
    /// for a group whose processes have all exited and are not yet reaped (zombies): EPERM means
    /// no process in the group could be signalled. Once the command has exited, that is what it
    /// means for its group, so it is not an error. Before, the command may have exited with the
    /// waiter not yet past its reap, so the failure is held, and is the run's error only if the
    /// command never exits (a command that changed its user, which xtask cannot stop).
    fn stop(&mut self, stop: Stop) {
        let Err(error) = self.send(stop) else {
            return;
        };
        if error.kind() != io::ErrorKind::PermissionDenied {
            self.keep(self.not_stopped(&error));
        } else if !self.exited {
            self.denied.get_or_insert(error);
        }
    }

    /// Sends `stop` to the tree, or lets a test's `STOP_HOOK` decide what it does.
    fn send(&self, stop: Stop) -> io::Result<()> {
        #[cfg(test)]
        if let Some(result) =
            STOP_HOOK.with_borrow_mut(|hook| hook.as_mut().map(|hook| hook(stop, &self.tree)))
        {
            return result;
        }
        match stop {
            Stop::Terminate => self.tree.terminate(),
            Stop::Kill => self.tree.kill(),
        }
    }

    fn enter(&mut self, phase: Phase, time: Duration) {
        self.phase = phase;
        self.left = time;
    }

    /// Keeps `error` unless an earlier one is kept.
    fn keep(&mut self, error: io::Error) {
        self.error.get_or_insert(error);
    }

    /// The error of a signal to the tree that failed.
    fn not_stopped(&self, error: &io::Error) -> io::Error {
        io::Error::other(format!("`{}` could not be stopped: {error}", self.shown))
    }
}

impl Drop for Running {
    /// A run dropped before its end (an error, a panic in `on_output`) stops its tree as a
    /// timeout does, then waits as `finish` would.
    fn drop(&mut self) {
        if !self.ended {
            // `Running` until the exit or the limit, so only a command still running is stopped.
            if self.phase == Phase::Running {
                self.terminate();
            }
            self.wait(&mut |_, _| {});
            self.tree.finish();
        }
    }
}

/// Starts a thread of a run on `body`.
fn start_thread(body: impl FnOnce() + Send + 'static) -> io::Result<()> {
    #[cfg(test)]
    BEFORE_THREAD.with_borrow_mut(|hook| hook.as_mut().map_or(Ok(()), |hook| hook()))?;
    thread::Builder::new().spawn(body).map(drop)
}

/// A hook a test sets on its own thread, run before each thread a run starts there; an error
/// from it is that thread's start failing.
#[cfg(test)]
pub(super) type ThreadHook = Box<dyn FnMut() -> io::Result<()>>;

/// A hook a test sets on its own thread, run in place of each stop a run sends its tree there:
/// it may send it (`Tree::terminate` or `Tree::kill`) or not, and says what the stop gave.
#[cfg(test)]
pub(super) type StopHook = Box<dyn FnMut(Stop, &OsTree) -> io::Result<()>>;

#[cfg(test)]
thread_local! {
    pub(super) static BEFORE_THREAD: std::cell::RefCell<Option<ThreadHook>> =
        const { std::cell::RefCell::new(None) };
    pub(super) static STOP_HOOK: std::cell::RefCell<Option<StopHook>> =
        const { std::cell::RefCell::new(None) };
}

/// Reads `reader` to its end, sending each chunk, and `ReadFailed` if a read fails. Returning
/// drops the thread's sender, which is how the run learns the pipe is closed.
fn read_all(stream: Stream, mut reader: io::PipeReader, sender: &Sender<Event>) {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                // The receiver is gone once the run has ended, when nobody wants the rest.
                if sender
                    .send(Event::Data(stream, chunk[..n].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
            Err(error) if interrupted(&error) => {}
            Err(error) => {
                drop(sender.send(Event::ReadFailed(error)));
                return;
            }
        }
    }
}

/// Whether a read failed only because a signal interrupted it (EINTR), so it is tried again.
fn interrupted(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
}
