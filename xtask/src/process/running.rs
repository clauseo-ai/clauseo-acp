//! A started command and the wait for it: one thread reads each output pipe, one waits for the
//! exit, and the caller's thread takes their events from one channel. The phases of the wait are
//! timed with `clock::timed` around each receive, so no timer thread and no sleep is needed.
//!
//! The run ends when the command has exited and its pipes are closed. At its limit its tree gets
//! SIGTERM (`Tree::terminate`), and SIGKILL after `KILL_GRACE` if the command or a pipe is still
//! open. A process that left the tree can hold a pipe for ever, so after the exit, or after
//! SIGKILL, the run waits at most `DRAIN` for the pipes. When the run ends, or its `Running` is
//! dropped, what is left of the tree gets SIGKILL (`Tree::finish`); a drop before the end stops it
//! the same way as a timeout first.

use super::{Cmd, spawn};
use crate::clock::timed;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use super::unix::Tree;
#[cfg(windows)]
use super::windows::Tree;

/// How long a tree has between SIGTERM and SIGKILL, as in `ui/app/scripts/buildtime-proc.mjs`.
pub(super) const KILL_GRACE: Duration = Duration::from_secs(5);
/// How long a run waits for the output pipes once the command has exited or its tree got SIGKILL.
const DRAIN: Duration = Duration::from_secs(1);
/// Size of each read from a running command's output pipe.
const CHUNK: usize = 8 * 1024;

/// Held while the pipes of a command are made and it starts. On macOS std makes a pipe and only
/// then marks its ends close-on-exec (`std/src/sys/pipe/unix.rs`), so a command started by
/// another thread in between would hold them and keep the reader from seeing EOF.
static STARTING: Mutex<()> = Mutex::new(());

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

/// How a run ended.
#[derive(Debug)]
pub(super) enum Finish {
    Exited(ExitStatus),
    /// It was still running at its limit, so its tree was stopped.
    TimedOut,
}

/// What the threads of a run send it.
enum Event {
    Data(Stream, Vec<u8>),
    /// A pipe reached EOF.
    Closed,
    /// A pipe could not be read; it counts as closed.
    ReadFailed(io::Error),
    Exited(io::Result<ExitStatus>),
}

/// Where a run is between its start and its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before its limit.
    Running,
    /// Exited before its limit; the pipes have `DRAIN` to close.
    Draining,
    /// Got SIGTERM at its limit, or at a drop; has `KILL_GRACE` to exit and close the pipes.
    Terminating,
    /// Got SIGKILL; the pipes have `DRAIN` to close.
    Killed,
}

/// A started command, until its run ends.
pub(super) struct Running {
    tree: Tree,
    /// The command as people see it, for the errors.
    shown: String,
    events: Receiver<Event>,
    /// Pipes not closed yet.
    open: usize,
    exited: bool,
    status: Option<ExitStatus>,
    phase: Phase,
    /// What is left of the current phase.
    left: Duration,
    timed_out: bool,
    /// The first error of the run: a pipe that could not be read, a failed wait or a failed stop.
    error: Option<io::Error>,
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
    Running::start(command, &cmd.to_string(), streams, cmd.timeout)?.finish(on_output)
}

impl Running {
    /// Starts `command` with stdin closed, in a tree of its own, with stdout and stderr as
    /// `streams` says, and `limit` to run. `shown` names it in errors.
    pub(super) fn start(
        mut command: Command,
        shown: &str,
        streams: Streams,
        limit: Duration,
    ) -> io::Result<Running> {
        command.stdin(Stdio::null());
        let starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        // Started before the command, so registering its group right after its start is one
        // write. Under `STARTING`, like every pipe.
        #[cfg(unix)]
        super::leader::ensure()?;
        Tree::prepare(&mut command);
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
        let tree = match Tree::adopt(&mut child) {
            Ok(tree) => tree,
            Err(error) => {
                // `adopt` stopped what it could not adopt; reap it.
                drop(child.wait());
                return Err(error);
            }
        };
        drop(starting);
        let (sender, events) = mpsc::channel();
        // From here a drop stops the tree, so a thread that cannot start stops it too.
        let running = Running {
            tree,
            shown: shown.to_owned(),
            events,
            open: readers.len(),
            exited: false,
            status: None,
            phase: Phase::Running,
            left: limit,
            timed_out: false,
            error: None,
            ended: false,
        };
        let waiter = sender.clone();
        thread::Builder::new().spawn(move || {
            // The receiver is gone once the run has ended, when nobody wants the status any more.
            drop(waiter.send(Event::Exited(child.wait())));
        })?;
        for (stream, reader) in readers {
            let sender = sender.clone();
            thread::Builder::new().spawn(move || read_all(stream, reader, &sender))?;
        }
        Ok(running)
    }

    /// Waits for the run to end, passing each chunk to `on_output`, and stops what is left of the
    /// tree. A failed read, wait or stop is the error; else a run past its limit is `TimedOut`.
    pub(super) fn finish(mut self, on_output: &mut dyn FnMut(Stream, &[u8])) -> io::Result<Finish> {
        self.wait(on_output);
        self.ended = true;
        self.tree.finish();
        match (self.error.take(), self.timed_out, self.status) {
            (Some(error), _, _) => Err(error),
            (None, true, _) => Ok(Finish::TimedOut),
            (None, false, Some(status)) => Ok(Finish::Exited(status)),
            // A run ends before its limit only after the exit, which sets the status or an error.
            (None, false, None) => Err(io::Error::other(format!(
                "the wait for `{}` ended without its exit status",
                self.shown
            ))),
        }
    }

    /// Takes events until the command has exited and its pipes are closed, or the last phase
    /// ran out.
    fn wait(&mut self, on_output: &mut dyn FnMut(Stream, &[u8])) {
        while !(self.exited && self.open == 0) {
            let (event, secs) = timed(|| self.events.recv_timeout(self.left));
            self.left = self.left.saturating_sub(Duration::from_secs_f64(secs));
            match event {
                Ok(Event::Data(stream, chunk)) => on_output(stream, &chunk),
                Ok(Event::Closed) => self.open -= 1,
                Ok(Event::ReadFailed(error)) => {
                    self.open -= 1;
                    self.keep(error);
                }
                Ok(Event::Exited(status)) => {
                    self.exited = true;
                    match status {
                        Ok(status) => self.status = Some(status),
                        Err(error) => self.keep(error),
                    }
                    if self.phase == Phase::Running {
                        self.enter(Phase::Draining, DRAIN);
                    }
                }
                Err(RecvTimeoutError::Timeout) => match self.phase {
                    Phase::Running => {
                        self.timed_out = true;
                        self.terminate();
                    }
                    Phase::Terminating => {
                        self.enter(Phase::Killed, DRAIN);
                        if let Err(error) = self.tree.kill() {
                            self.keep(self.not_stopped(&error));
                        }
                    }
                    Phase::Draining | Phase::Killed => return,
                },
                // Every thread has ended, so the exit and every EOF are in.
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Sends the tree SIGTERM and gives it `KILL_GRACE`.
    fn terminate(&mut self) {
        self.enter(Phase::Terminating, KILL_GRACE);
        if let Err(error) = self.tree.terminate() {
            self.keep(self.not_stopped(&error));
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
            if !self.exited && self.phase == Phase::Running {
                self.terminate();
            }
            self.wait(&mut |_, _| {});
            self.tree.finish();
        }
    }
}

/// Reads `reader` to its end, sending each chunk, then `Closed` or `ReadFailed`.
fn read_all(stream: Stream, mut reader: io::PipeReader, sender: &Sender<Event>) {
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let event = match reader.read(&mut chunk) {
            Ok(0) => Event::Closed,
            Ok(n) => Event::Data(stream, chunk[..n].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => Event::ReadFailed(error),
        };
        let last = !matches!(event, Event::Data(..));
        // The receiver is gone once the run has ended, when nobody wants the rest.
        if sender.send(event).is_err() || last {
            return;
        }
    }
}
