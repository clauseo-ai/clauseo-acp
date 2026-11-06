//! The child and the two pumps. agent-browser starts with the map `env.rs` built, in `--cwd`,
//! with piped stdin and stdout and the guard's stderr. One thread copies its stdout to the
//! agent, whole lines and unchanged bytes; another reads the agent's lines, runs each through
//! `filter.rs`, and writes what passes to the child. Both write to the guard's stdout only under
//! one lock, a whole line at a time, so a refusal never lands inside a line of the child's.
//!
//! When the agent closes stdin, the guard asks agent-browser to close its browser (one
//! `agent_browser_close` call, whose reply is not relayed; for the guard's Chrome that only
//! disconnects), waits up to [`CLOSE_WAIT`] for that reply, closes the child's stdin and waits
//! up to [`EXIT_WAIT`] for its stdout to end, then kills it. When the child ends first, the
//! guard exits at once. When Chrome exits (`chrome.rs`) or the guard's parent goes
//! (`parent.rs`), agent-browser is ended at once and the guard exits 1. Chrome is also looked at
//! once the relay ends: had it died unnoticed (between two of its watcher's looks) while
//! agent-browser ended, the guard still exits 1. Whatever ends the relay, `browser_guard.rs`
//! then ends agent-browser's detached daemon and Chrome.
//!
//! The child's lines are capped at [`CHILD_MAX_LINE_BYTES`]: a longer one kills the child and
//! the guard exits 1 (fail closed). A last line cut off by the child's end is dropped, with a
//! note on stderr, so the agent never gets half a JSON message.
//!
//! After each line it relays, the stdout pump checks the port files (`port_file.rs`): the stream
//! server's on every OS and the daemon's on Windows, for the session's whole life. A listener on
//! any other port ends agent-browser and the guard exits 1. On Windows agent-browser runs in the
//! Job Object the guard owns (`job.rs`) beside Chrome, so that end, and the guard's own exit,
//! take its detached daemon too. On Unix agent-browser leads a process group of its own, and
//! every end, its own exit included, kills that group before agent-browser is reaped: a CLI
//! call of its that still runs could otherwise start a daemon after the guard's cleanup. The
//! daemon itself leaves the group (`setsid`), and `reaper.rs`'s `kill_daemon` ends it
//! afterwards; the reaper also learns agent-browser's pid, for a guard that is killed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::chrome::Chrome;
use super::filter::{self, MAX_LINE_BYTES, RESERVED_ID as CLOSE_ID, Verdict};
use super::note;
use super::parent::Parent;
use super::port_file::{Expected, Status};
/// The request sent when the agent closes stdin.
pub(crate) const CLOSE_REQUEST: &str = r#"{"jsonrpc":"2.0","id":"clauseo-guard-close","method":"tools/call","params":{"name":"agent_browser_close","arguments":{}}}"#;
/// How long the guard waits for the close reply before closing the child's stdin anyway.
pub(crate) const CLOSE_WAIT: Duration = Duration::from_secs(10);
/// How long the guard waits for the child's stdout to end after closing its stdin, before it
/// kills the child.
pub(crate) const EXIT_WAIT: Duration = Duration::from_secs(5);
/// The longest child line the guard relays, in bytes without the newline.
pub(crate) const CHILD_MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Windows: the child gets no console window (agent-browser issue #1644).
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The guard's stdout, shared by both pumps.
type Out = Arc<Mutex<io::Stdout>>;
/// The child's stdin; `None` once closed.
type ToChild = Arc<Mutex<Option<ChildStdin>>>;

enum Event {
    /// The child's stdout reached EOF.
    ChildEnded,
    /// The child wrote a line over [`CHILD_MAX_LINE_BYTES`].
    ChildOversize,
    /// The agent's stdin reached EOF and the close request went out.
    AgentEnded,
    /// The reply to the close request arrived.
    CloseAnswered,
    /// A listener is not on its claimed port; the reason.
    PortRefused(String),
    /// The guard's Chrome exited.
    ChromeExited,
    /// The guard's parent has gone.
    ParentGone,
}

/// How the relay ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The child exited (or was killed after [`EXIT_WAIT`]) with this code; 1 for a signal.
    Exited(i32),
    /// The child wrote a line over [`CHILD_MAX_LINE_BYTES`] and was killed.
    ChildLineTooLong,
    /// A port file named another port, or could not be trusted, and agent-browser was ended
    /// (its whole job on Windows). The reason.
    PortRefused(String),
    /// Chrome exited and agent-browser was ended.
    ChromeExited,
    /// The guard's parent went and agent-browser was ended.
    ParentGone,
}

/// What [`wait_for_end`] saw.
enum End {
    ChildEnded,
    ChildOversize,
    ExitTimedOut,
    PortRefused(String),
    ChromeExited,
    ParentGone,
}

/// What agent-browser runs as, and beside what.
pub(crate) struct Agent<'a> {
    pub(crate) program: &'a Path,
    pub(crate) args: &'a [OsString],
    pub(crate) cwd: &'a Path,
    pub(crate) env: &'a BTreeMap<String, OsString>,
    /// Checked after every relayed line.
    pub(crate) port_checks: Vec<Expected>,
    /// Given agent-browser's pid (on Unix also its process group's id) once it runs: the guard
    /// tells the reaper.
    pub(crate) on_spawn: &'a mut dyn FnMut(u32),
    /// Called once the guard has killed agent-browser's group, just before it reaps
    /// agent-browser: the guard tells the reaper, which then leaves that group alone.
    pub(crate) on_reap: &'a mut dyn FnMut(),
}

/// Starts agent-browser and relays until it exits, Chrome exits or the guard's parent goes.
pub(crate) fn run(
    spec: Agent<'_>,
    chrome: &mut Chrome,
    parent: &Parent,
    #[cfg(windows)] job: &super::job::Job,
) -> io::Result<Outcome> {
    let Agent {
        program,
        args,
        cwd,
        env,
        port_checks,
        on_spawn,
        on_reap,
    } = spec;
    // On Windows in the guard's job, which ends what is left of the tree when the guard exits.
    #[cfg(windows)]
    let mut child = {
        let mut child = spawn_child(program, args, cwd, env)?;
        job.adopt(&mut child, "agent-browser")?;
        Tree {
            child,
            job,
            on_reap,
        }
    };
    #[cfg(not(windows))]
    let mut child = Tree {
        child: spawn_child(program, args, cwd, env)?,
        job: std::marker::PhantomData,
        on_reap,
    };
    on_spawn(child.child.id());
    let stdout = child
        .child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("the child's stdout is not piped"))?;
    let to_child: ToChild = Arc::new(Mutex::new(child.child.stdin.take()));
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let closing = Arc::new(AtomicBool::new(false));
    let (events, inbox) = mpsc::channel();
    {
        let events = events.clone();
        chrome.watch(move || send(&events, Event::ChromeExited));
    }
    {
        let events = events.clone();
        parent.watch(move || send(&events, Event::ParentGone));
    }

    let child_pump = {
        let (out, closing, events) = (Arc::clone(&out), Arc::clone(&closing), events.clone());
        thread::spawn(move || {
            let end = pump_child(
                BufReader::new(stdout),
                &out,
                &closing,
                &events,
                &port_checks,
            );
            send(&events, end);
        })
    };
    {
        let (out, to_child) = (Arc::clone(&out), Arc::clone(&to_child));
        // Never joined: when the child ends first, the guard exits with this thread still
        // blocked on the agent's stdin.
        drop(thread::spawn(move || {
            pump_agent(&mut io::stdin().lock(), &to_child, &out, &closing, &events);
        }));
    }

    let end = wait_for_end(&inbox, &to_child);
    // Before anything here ends Chrome (the job's end on Windows): a Chrome that is gone now
    // died by itself.
    let chrome_died = chrome.exited();
    match end {
        End::ChildEnded => {
            if child_pump.join().is_err() {
                return Err(io::Error::other("the stdout relay thread panicked"));
            }
            let code = child.reap()?.code().unwrap_or(1);
            if chrome_died || chrome.exited() {
                return Ok(Outcome::ChromeExited);
            }
            Ok(Outcome::Exited(code))
        }
        End::ChildOversize => {
            stop(&mut child)?;
            Ok(Outcome::ChildLineTooLong)
        }
        End::PortRefused(reason) => {
            stop(&mut child)?;
            Ok(Outcome::PortRefused(reason))
        }
        End::ChromeExited => {
            stop(&mut child)?;
            Ok(Outcome::ChromeExited)
        }
        End::ParentGone => {
            stop(&mut child)?;
            Ok(Outcome::ParentGone)
        }
        End::ExitTimedOut => {
            note(&format!(
                "agent-browser did not exit within {} s of its stdin closing; killed it",
                EXIT_WAIT.as_secs()
            ));
            let code = stop(&mut child)?.code().unwrap_or(1);
            if chrome_died {
                return Ok(Outcome::ChromeExited);
            }
            Ok(Outcome::Exited(code))
        }
    }
}

/// agent-browser, and on Windows the job that holds it, all it starts, and Chrome.
struct Tree<'a> {
    child: Child,
    #[cfg(windows)]
    job: &'a super::job::Job,
    #[cfg(not(windows))]
    job: std::marker::PhantomData<&'a ()>,
    on_reap: &'a mut dyn FnMut(),
}

impl Tree<'_> {
    /// Ends agent-browser's group ([`end_group`]), calls the reap hook, and reaps agent-browser.
    fn reap(&mut self) -> io::Result<std::process::ExitStatus> {
        end_group(&mut self.child);
        (self.on_reap)();
        self.child.wait()
    }
}

/// Ends agent-browser and reaps it: on Windows the whole job (its daemon and Chrome too), on
/// Unix its process group (it may have exited already). The stdout pump is not joined: a
/// grandchild could still hold the pipe open.
fn stop(tree: &mut Tree<'_>) -> io::Result<std::process::ExitStatus> {
    #[cfg(windows)]
    if !tree.job.kill() {
        note("Windows refused to end agent-browser's job; killing agent-browser alone");
    }
    tree.reap()
}

/// Kills agent-browser and, on Unix, whatever is left in its process group. Called only while
/// agent-browser is unreaped, so the group's id (its pid) cannot have been reused; a child that
/// exited already keeps its exit code.
fn end_group(child: &mut Child) {
    #[cfg(unix)]
    super::chrome::kill_group(child.id());
    #[cfg(not(unix))]
    // It may have exited already; nothing is left to kill then.
    drop(child.kill());
}

/// The phase of the shutdown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Relaying; nothing has ended.
    Running,
    /// The agent has gone and the close request is out; waiting for its reply.
    Closing,
    /// The child's stdin is closed; waiting for its stdout to end.
    Draining,
}

/// Returns once the child's stdout has ended, it wrote an oversize line, or it outlived
/// [`EXIT_WAIT`] after its stdin was closed (following the agent's EOF and the close reply, or
/// [`CLOSE_WAIT`]). Each wait restarts on an event, but each phase admits only a bounded number
/// of them: the close reply is sent at most twice (by the reply, or by a failed write).
fn wait_for_end(inbox: &mpsc::Receiver<Event>, to_child: &ToChild) -> End {
    let mut phase = Phase::Running;
    loop {
        let event = match phase {
            Phase::Running => inbox.recv().unwrap_or(Event::ChildEnded),
            Phase::Closing => match inbox.recv_timeout(CLOSE_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => Event::CloseAnswered,
                Err(RecvTimeoutError::Disconnected) => Event::ChildEnded,
            },
            Phase::Draining => match inbox.recv_timeout(EXIT_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => return End::ExitTimedOut,
                Err(RecvTimeoutError::Disconnected) => Event::ChildEnded,
            },
        };
        match event {
            Event::ChildEnded => return End::ChildEnded,
            Event::ChildOversize => return End::ChildOversize,
            Event::PortRefused(reason) => return End::PortRefused(reason),
            Event::ChromeExited => return End::ChromeExited,
            Event::ParentGone => return End::ParentGone,
            Event::AgentEnded => {
                if phase == Phase::Running {
                    phase = Phase::Closing;
                }
            }
            Event::CloseAnswered => {
                if phase != Phase::Draining {
                    phase = Phase::Draining;
                    drop(lock(to_child).take());
                }
            }
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the guard's one fixed child: agent-browser, whose tree the host kills; the host \
              starts the guard itself through the process crate (docs/decisions/0029-browser.md)"
)]
fn spawn_child(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &BTreeMap<String, OsString>,
) -> io::Result<Child> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A group of its own, which `end_group` kills whole.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Suspended until it is in its job (`job.rs`).
        command.creation_flags(CREATE_NO_WINDOW | super::job::START_SUSPENDED);
    }
    command.spawn()
}

/// Child to agent: each whole line as read, the reply to the close request excepted. After
/// each line, every one of `port_checks`. Returns the event that ends it:
/// [`Event::ChildEnded`], [`Event::ChildOversize`] or [`Event::PortRefused`].
fn pump_child(
    mut stdout: impl BufRead,
    out: &Out,
    closing: &AtomicBool,
    events: &mpsc::Sender<Event>,
    port_checks: &[Expected],
) -> Event {
    let mut line = Vec::new();
    // Once the agent stops reading, the child's output is still drained so it never blocks.
    let mut agent_reads = true;
    loop {
        match read_line(&mut stdout, &mut line, CHILD_MAX_LINE_BYTES, Overflow::Stop) {
            Ok(Line::Eof) | Err(_) => return Event::ChildEnded,
            // `browser_guard::run` says so on stderr once the child is killed.
            Ok(Line::Oversize) => return Event::ChildOversize,
            Ok(Line::Partial) => {
                note(&format!(
                    "agent-browser ended in the middle of a line; dropped its last {} bytes",
                    line.len()
                ));
                return Event::ChildEnded;
            }
            Ok(Line::Whole) => {}
        }
        if closing.load(Ordering::SeqCst) && is_close_reply(&line) {
            send(events, Event::CloseAnswered);
            continue;
        }
        line.push(b'\n');
        if agent_reads && emit(out, &line).is_err() {
            agent_reads = false;
        }
        if let Some(reason) = refusal(port_checks) {
            return Event::PortRefused(reason);
        }
    }
}

/// The first refusal among `checks`, each read now; none while every file is missing, empty or
/// on its claimed port. Never stops checking: a match now says nothing about the next line.
pub(crate) fn refusal(checks: &[Expected]) -> Option<String> {
    checks.iter().find_map(|check| match check.check() {
        Status::Pending | Status::Matches => None,
        Status::Refused(reason) => Some(reason),
    })
}

fn is_close_reply(line: &[u8]) -> bool {
    serde_json::from_slice::<Value>(line).is_ok_and(|reply| {
        reply.get("id").and_then(Value::as_str) == Some(CLOSE_ID) && reply.get("method").is_none()
    })
}

/// Agent to child: every line through the filter. On EOF, sends the close request.
fn pump_agent(
    stdin: &mut impl BufRead,
    to_child: &ToChild,
    out: &Out,
    closing: &AtomicBool,
    events: &mpsc::Sender<Event>,
) {
    let mut line = Vec::new();
    loop {
        let verdict = match read_line(stdin, &mut line, MAX_LINE_BYTES, Overflow::Skip) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Oversize) => filter::parse_error(),
            // The agent's last line may lack its newline; it is checked like any other.
            Ok(Line::Whole | Line::Partial) => filter::check(&line),
        };
        let sent = match verdict {
            Verdict::Forward(text) => write_child(to_child, &text),
            // The agent has gone if its stdout fails; its stdin EOF follows.
            Verdict::Answer(text) => emit(out, format!("{text}\n").as_bytes()).or(Ok(())),
            Verdict::Drop => Ok(()),
        };
        if sent.is_err() {
            // The child has gone; the stdout pump sees its EOF and the guard exits.
            return;
        }
    }
    closing.store(true, Ordering::SeqCst);
    send(events, Event::AgentEnded);
    if write_child(to_child, CLOSE_REQUEST).is_err() {
        send(events, Event::CloseAnswered);
    }
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line ended by a newline, now in the buffer without it.
    Whole,
    /// Bytes the input ended in without a newline, now in the buffer.
    Partial,
    /// A line over the limit, thrown away (read to its end with [`Overflow::Skip`]).
    Oversize,
    /// The end of input, nothing read.
    Eof,
}

/// What [`read_line`] does with a line over its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overflow {
    /// Read it to its end and throw it away, so the next line can be read.
    Skip,
    /// Return at once, leaving the rest unread: the stream is not read again.
    Stop,
}

/// Reads one line into `buf` (cleared first), never holding more than `max` bytes of it.
pub(crate) fn read_line(
    input: &mut impl BufRead,
    buf: &mut Vec<u8>,
    max: usize,
    overflow: Overflow,
) -> io::Result<Line> {
    buf.clear();
    let mut over = false;
    let mut any = false;
    loop {
        let chunk = match input.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            return Ok(match (over, any) {
                (true, _) => Line::Oversize,
                (false, true) => Line::Partial,
                (false, false) => Line::Eof,
            });
        }
        any = true;
        let newline = chunk.iter().position(|&b| b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if !over {
            if buf.len() + take > max {
                if overflow == Overflow::Stop {
                    buf.clear();
                    return Ok(Line::Oversize);
                }
                over = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        input.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(if over { Line::Oversize } else { Line::Whole });
        }
    }
}

/// Writes one whole line to the child, newline included, in one call under its lock.
fn write_child(to_child: &ToChild, text: &str) -> io::Result<()> {
    let mut guard = lock(to_child);
    let stdin = guard
        .as_mut()
        .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
    let mut line = Vec::with_capacity(text.len() + 1);
    line.extend_from_slice(text.as_bytes());
    line.push(b'\n');
    stdin.write_all(&line)?;
    stdin.flush()
}

/// Writes whole lines to the guard's stdout under the shared lock.
fn emit(out: &Out, bytes: &[u8]) -> io::Result<()> {
    let mut stdout = lock(out);
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A pump that panicked holding the lock leaves whole lines behind; carry on.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(events: &mpsc::Sender<Event>, event: Event) {
    // The receiver goes only when the guard is returning; nothing waits for this event then.
    if events.send(event).is_err() {}
}

#[cfg(test)]
mod port_tests;

#[cfg(test)]
mod tests;
