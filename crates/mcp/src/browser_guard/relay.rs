//! The relay between the agent and agent-browser, and the browser's lifecycle around it.
//!
//! One thread reads the agent's lines, runs each through `filter.rs` and routes what passes
//! (`pumps.rs`): to the running agent-browser; a tool call while the browser is stopped starts
//! Chrome and agent-browser first (`session.rs`); any other request then goes to a one-shot
//! agent-browser (`oneshot.rs`). Every start of agent-browser has a thread that copies its stdout
//! to the agent, whole lines and unchanged bytes. All of them write to the guard's stdout only
//! under one lock, a whole line at a time, so a refusal never lands inside a line of the child's.
//! agent-browser starts with the map `env.rs` built plus Chrome's endpoint, in `--cwd`, with
//! piped stdin and stdout and the guard's stderr.
//!
//! This thread waits for the events. With nothing in flight for [`session::IDLE_STOP`] after the
//! last tool call it stops the browser, keeping the profile; the next call starts it again. The
//! agent's own `agent_browser_close` stops it the same way once its reply has gone to the agent
//! ([`Event::Closed`]), unless a request went to agent-browser since: agent-browser only
//! disconnects from a Chrome it did not launch, so without this Chrome and its loopback debugging
//! port lived until the idle stop, and the port now lives only while the agent uses the browser
//! (open to close within a turn; Codex closes at the end of every turn). A close while the browser
//! is stopped starts nothing: the guard answers it with agent-browser's own result
//! ([`CLOSED_RESULT`]); one that waited for a start in progress goes to the started browser and
//! stops it again. A pipe instead of the port is for after the launch.
//!
//! When Chrome dies the first time, the relay stops agent-browser too, answers each request in
//! flight with an error, and the next call starts both again; the second time the guard exits 1.
//! When the agent closes stdin, the guard asks a running agent-browser to close its browser (one
//! `agent_browser_close` call, [`CLOSE_REQUEST`], whose reply is not relayed and stops nothing
//! itself: the final stop follows), waits up to [`CLOSE_WAIT`] for that reply, closes the child's
//! stdin and waits up to [`EXIT_WAIT`] for its stdout to end, then kills it. When agent-browser ends by itself
//! the guard exits with its code (1 if Chrome had died too). When the guard's parent goes
//! (`parent.rs`), the guard exits 1. Whatever ends the relay, the browser is stopped (`stop`), a
//! running one-shot killed, and every request still in flight answered with an error. From that
//! final stop on, the agent pump answers each request with an error itself: no browser or
//! one-shot starts after it, so none outlives the guard.
//!
//! The child's lines are capped at [`CHILD_MAX_LINE_BYTES`]: a longer one kills the child and
//! the guard exits 1 (fail closed). A last line cut off by the child's end is dropped, with a
//! note on stderr, so the agent never gets half a JSON message.
//!
//! After each line it relays, the stdout pump checks the port files (`port_file.rs`): the stream
//! server's on every OS and the daemon's on Windows, for the session's whole life. A listener on
//! any other port ends agent-browser and the guard exits 1. On Windows agent-browser runs in the
//! Job Object the guard owns (`job.rs`) beside Chrome, so a stop, and the guard's own exit, take
//! its detached daemon too. On Unix agent-browser leads a process group of its own, and every
//! stop kills that group before agent-browser is reaped: a CLI call of its that still runs could
//! otherwise start a daemon after the guard's cleanup. The daemon itself leaves the group
//! (`setsid`), and `reaper.rs`'s `kill_daemon` ends it afterwards; the reaper also learns
//! agent-browser's pid, for a guard that is killed.

pub(crate) mod lines;
mod oneshot;
pub(crate) mod pumps;

use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use super::parent::Parent;
use super::session::{self, Shared, Stopped};
use super::{Child, note};
pub(crate) use lines::{Line, Overflow, read_line};
use lines::{answer_all, lock};

/// The request sent when the agent closes stdin.
pub(crate) const CLOSE_REQUEST: &str = r#"{"jsonrpc":"2.0","id":"clauseo-guard-close","method":"tools/call","params":{"name":"agent_browser_close","arguments":{}}}"#;
/// agent-browser's tool that closes its browser. Its reply to the agent stops the guard's
/// browser (`Event::Closed`); while the browser is stopped the guard answers it itself.
pub(crate) const CLOSE_TOOL: &str = "agent_browser_close";
/// agent-browser v0.41.1's result for `agent_browser_close` on the guard's Chrome, the same JSON
/// as it sends, which the guard sends for a close while its browser is stopped. The real-Chrome
/// test `real_chrome::close::a_close_while_stopped_starts_nothing_and_gets_agent_browsers_result`
/// compares it with the real binary's, so a bump that changes it fails there.
const CLOSED_RESULT: &str = concat!(
    r##"{"content":[{"text":"{\n  \"data\": {\n    \"closed\": true,\n    \"lifecycle\":"##,
    r##" {\n      \"effectiveLaunch\": {\n        \"browserLaunched\": false,\n        \"engine\":"##,
    r##" \"chrome\",\n        \"launchHash\": null\n      },\n      \"launched\": false,"##,
    r##"\n      \"relaunchedBrowser\": false,\n      \"restartedBackground\": false,\n      \"restoreStatus\":"##,
    r##" \"not_configured\",\n      \"reused\": false,\n      \"saveStatus\": \"not_configured\"\n    },"##,
    r##"\n    \"restoreStatus\": \"not_configured\",\n    \"saveStatus\": \"not_configured\"\n  },"##,
    r##"\n  \"error\": null,\n  \"success\": true\n}","type":"text"}],"isError":false,"structuredContent":"##,
    r##"{"exitCode":0,"response":{"success":true,"data":{"closed":true,"lifecycle":{"effectiveLaunch":"##,
    r##"{"browserLaunched":false,"engine":"chrome","launchHash":null},"launched":false,"relaunchedBrowser":"##,
    r##"false,"restartedBackground":false,"restoreStatus":"not_configured","reused":false,"##,
    r##""saveStatus":"not_configured"},"restoreStatus":"not_configured","saveStatus":"not_configured"},"##,
    r##""error":null},"stderr":"","stdout":"{\"success\":true,\"data\":{\"closed\":true,"##,
    r##"\"lifecycle\":{\"effectiveLaunch\":{\"browserLaunched\":false,\"engine\":\"chrome\","##,
    r##"\"launchHash\":null},\"launched\":false,\"relaunchedBrowser\":false,\"restartedBackground\":"##,
    r##"false,\"restoreStatus\":\"not_configured\",\"reused\":false,\"saveStatus\":\"not_configured\"},"##,
    r##"\"restoreStatus\":\"not_configured\",\"saveStatus\":\"not_configured\"},\"error\":"##,
    r##"null}\n"}}"##,
);

/// The guard's reply to an `agent_browser_close` with `id` while its browser is stopped: the
/// result agent-browser gives, so the agent sees no difference. With its newline.
pub(crate) fn closed_reply(id: &serde_json::Value) -> String {
    let result: serde_json::Value =
        serde_json::from_str(CLOSED_RESULT).unwrap_or(serde_json::Value::Null);
    format!(
        "{}\n",
        serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result})
    )
}

/// How long the guard waits for the close reply before closing the child's stdin anyway.
pub(crate) const CLOSE_WAIT: Duration = Duration::from_secs(10);
/// How long the guard waits for the child's stdout to end after closing its stdin, before it
/// kills the child.
pub(crate) const EXIT_WAIT: Duration = Duration::from_secs(5);
/// The longest child line the guard relays, in bytes without the newline.
pub(crate) const CHILD_MAX_LINE_BYTES: usize = 64 * 1024 * 1024;
/// The longest the event loop waits before it looks at the idle time again.
const IDLE_LOOK: Duration = Duration::from_secs(1);

/// The error a request in flight gets when Chrome dies the first time.
const CRASHED: &str = "Chrome exited unexpectedly; the guard stopped the browser, and the next \
                       browser call starts it again";
/// The error a request in flight gets when the guard ends.
const ENDED: &str = "the browser server ended before this call was answered";

/// What the guard's threads report. The browser's events carry the generation of the start
/// they belong to (`session.rs`).
pub(crate) enum Event {
    /// agent-browser's stdout reached EOF.
    ChildEnded(u64),
    /// agent-browser wrote a line over [`CHILD_MAX_LINE_BYTES`].
    ChildOversize(u64),
    /// A listener is not on its claimed port; the reason.
    PortRefused(u64, String),
    /// The guard's Chrome exited.
    ChromeExited(u64),
    /// agent-browser's reply to an `agent_browser_close` call went to the agent.
    Closed(u64),
    /// The agent's stdin reached EOF (and the close request went out to a running child).
    AgentEnded,
    /// The reply to the close request arrived.
    CloseAnswered,
    /// The guard's parent has gone.
    ParentGone,
    /// The browser could not start for a tool call; the reason.
    StartFailed(String),
    /// A one-shot agent-browser could not start; the error.
    AgentFailed(String),
    /// A one-shot agent-browser ended the server with this code.
    OneShotEnded(i32),
    /// A one-shot agent-browser wrote a line over [`CHILD_MAX_LINE_BYTES`].
    OneShotTooLong,
}

/// How the relay ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// agent-browser exited (or was killed after [`EXIT_WAIT`]) with this code; 1 for a signal.
    /// 0 when the agent left while the browser was stopped.
    Exited(i32),
    /// The child wrote a line over [`CHILD_MAX_LINE_BYTES`] and was killed.
    ChildLineTooLong,
    /// A port file named another port, or could not be trusted, and agent-browser was ended
    /// (its whole job on Windows). The reason.
    PortRefused(String),
    /// Chrome exited a second time, or as the relay ended, and agent-browser was ended.
    ChromeExited,
    /// The guard's parent went and agent-browser was ended.
    ParentGone,
    /// The browser could not start; the reason.
    StartFailed(String),
    /// agent-browser could not start; the error.
    AgentFailed(String),
}

/// What [`wait_for_end`] saw.
enum End {
    ChildEnded,
    ChildOversize,
    ExitTimedOut,
    PortRefused(String),
    ChromeExited,
    ParentGone,
    /// The agent left with no agent-browser running.
    AgentGone,
    StartFailed(String),
    AgentFailed(String),
    OneShotEnded(i32),
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

/// Relays until the agent leaves, agent-browser exits, Chrome dies twice, the browser cannot
/// start or the guard's parent goes; then stops whatever runs.
pub(crate) fn run(shared: &Arc<Shared>, parent: &Parent) -> Outcome {
    let (events, inbox) = mpsc::channel();
    {
        let events = events.clone();
        parent.watch(move || send(&events, Event::ParentGone));
    }
    {
        let (shared, parent) = (Arc::clone(shared), parent.clone());
        // Never joined: when the child ends first, the guard exits with this thread still
        // blocked on the agent's stdin.
        drop(thread::spawn(move || {
            pumps::pump_agent(&mut io::stdin().lock(), &shared, &parent, &events);
        }));
    }
    let end = wait_for_end(shared, &inbox);
    let (stopped, in_flight) = {
        let mut state = lock(&shared.state);
        // Before the final stop, under the same lock: a request after it starts nothing (the
        // agent pump still reads stdin while the guard cleans up and exits).
        state.ended = true;
        let stopped = session::stop(&shared.launcher, &mut state);
        (stopped, state.take_in_flight())
    };
    drop(oneshot::end(shared));
    answer_all(&shared.out, &in_flight, ENDED);
    outcome(end, stopped)
}

/// The outcome of `end`, given what the final stop found.
fn outcome(end: End, stopped: Option<Stopped>) -> Outcome {
    let chrome_died = stopped.as_ref().is_some_and(|stopped| stopped.chrome_died);
    let code = stopped
        .and_then(|stopped| stopped.status.ok())
        .and_then(|status| status.code())
        .unwrap_or(1);
    match end {
        End::ChildEnded | End::ExitTimedOut if chrome_died => Outcome::ChromeExited,
        End::ChildEnded => Outcome::Exited(code),
        End::ExitTimedOut => {
            note(&format!(
                "agent-browser did not exit within {} s of its stdin closing; killed it",
                EXIT_WAIT.as_secs()
            ));
            Outcome::Exited(code)
        }
        End::AgentGone => Outcome::Exited(0),
        End::OneShotEnded(code) => Outcome::Exited(code),
        End::ChildOversize => Outcome::ChildLineTooLong,
        End::PortRefused(reason) => Outcome::PortRefused(reason),
        End::ChromeExited => Outcome::ChromeExited,
        End::ParentGone => Outcome::ParentGone,
        End::StartFailed(reason) => Outcome::StartFailed(reason),
        End::AgentFailed(error) => Outcome::AgentFailed(error),
    }
}

/// Returns once the relay must end. Each wait restarts on an event, but each shutdown phase
/// admits only a bounded number of them: the close reply is sent at most twice (by the reply, or
/// by a failed write). While relaying it looks at the idle time at least every [`IDLE_LOOK`].
fn wait_for_end(shared: &Shared, inbox: &mpsc::Receiver<Event>) -> End {
    let mut phase = Phase::Running;
    loop {
        let event = match phase {
            Phase::Running => match inbox.recv_timeout(idle_wait(shared)) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    stop_if_idle(shared);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return End::AgentGone,
            },
            Phase::Closing => match inbox.recv_timeout(CLOSE_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => Event::CloseAnswered,
                Err(RecvTimeoutError::Disconnected) => return End::AgentGone,
            },
            Phase::Draining => match inbox.recv_timeout(EXIT_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => return End::ExitTimedOut,
                Err(RecvTimeoutError::Disconnected) => return End::AgentGone,
            },
        };
        let (generation, live) = {
            let state = lock(&shared.state);
            (state.generation, state.live.is_some())
        };
        let current = |of: u64| of == generation;
        match event {
            Event::ChildEnded(of) if current(of) => return End::ChildEnded,
            Event::ChildOversize(of) if current(of) => return End::ChildOversize,
            Event::PortRefused(of, reason) if current(of) => return End::PortRefused(reason),
            Event::ChromeExited(of) if current(of) => {
                if phase != Phase::Running || !restart_later(shared) {
                    return End::ChromeExited;
                }
            }
            Event::Closed(of) if current(of) => stop_after_close(shared),
            // From a start the guard has stopped already.
            Event::Closed(_)
            | Event::ChildEnded(_)
            | Event::ChildOversize(_)
            | Event::PortRefused(..)
            | Event::ChromeExited(_) => {}
            Event::ParentGone => return End::ParentGone,
            Event::StartFailed(reason) => return End::StartFailed(reason),
            Event::AgentFailed(error) => return End::AgentFailed(error),
            Event::OneShotEnded(code) => return End::OneShotEnded(code),
            Event::OneShotTooLong => return End::ChildOversize,
            Event::AgentEnded => {
                if phase == Phase::Running {
                    phase = Phase::Closing;
                }
            }
            Event::CloseAnswered => {
                if phase != Phase::Draining {
                    phase = Phase::Draining;
                    if let Some(live) = lock(&shared.state).live.as_ref() {
                        drop(lock(&live.to_child).take());
                    }
                }
            }
        }
        // The browser stopped (Chrome died) while the agent was leaving: nothing to wait for.
        if phase != Phase::Running && !live {
            return End::AgentGone;
        }
    }
}

/// How long the event loop may wait now: until the browser has idled out, at most
/// [`IDLE_LOOK`].
fn idle_wait(shared: &Shared) -> Duration {
    lock(&shared.state)
        .idle_left(shared.launcher.idle)
        .map_or(IDLE_LOOK, |left| left.min(IDLE_LOOK))
}

/// Stops the browser if it has idled out.
fn stop_if_idle(shared: &Shared) {
    let mut state = lock(&shared.state);
    if state.idle_left(shared.launcher.idle) == Some(Duration::ZERO) {
        drop(session::stop(&shared.launcher, &mut state));
        note(&format!(
            "no browser call for {} s; stopped Chrome and agent-browser, keeping the profile \
             until the session ends",
            shared.launcher.idle.as_secs()
        ));
    }
}

/// The agent's `agent_browser_close` was answered: stops the browser as the idle stop does,
/// keeping the profile, so Chrome's debugging port lives only while the agent uses the browser.
/// Runs on this thread after the reply is out, so the stop (with its wait for Chrome's group,
/// `chrome.rs`) never delays the reply. Not when a request went to agent-browser since (the agent
/// browses again; the idle stop covers it) or the agent has gone (the final stop follows).
fn stop_after_close(shared: &Shared) {
    let mut state = lock(&shared.state);
    if state.quiet() {
        drop(session::stop(&shared.launcher, &mut state));
        drop(state);
        note(
            "agent_browser_close: stopped Chrome and agent-browser, keeping the profile until the \
             session ends",
        );
    }
}

/// Chrome died while relaying. The first time: stops agent-browser, answers what is in flight
/// with an error and returns `true`, so the next tool call starts both again. `false` the
/// second time, or once the agent has gone.
fn restart_later(shared: &Shared) -> bool {
    let in_flight = {
        let mut state = lock(&shared.state);
        if state.restarted || state.closing {
            return false;
        }
        state.restarted = true;
        drop(session::stop(&shared.launcher, &mut state));
        state.take_in_flight()
    };
    note(
        "Chrome exited; the guard ended agent-browser and starts both again at the next browser \
         call (once per session)",
    );
    answer_all(&shared.out, &in_flight, CRASHED);
    true
}

/// Kills agent-browser and, on Unix, whatever is left in its process group. Called only while
/// agent-browser is unreaped, so the group's id (its pid) cannot have been reused; a child that
/// exited already keeps its exit code.
pub(crate) fn end_group(child: &mut Child) {
    #[cfg(unix)]
    super::chrome::kill_group(child.id());
    #[cfg(not(unix))]
    // It may have exited already; nothing is left to kill then.
    drop(child.kill());
}

pub(crate) fn send(events: &mpsc::Sender<Event>, event: Event) {
    // The receiver goes only when the guard is returning; nothing waits for this event then.
    if events.send(event).is_err() {}
}

#[cfg(test)]
mod port_tests;

#[cfg(test)]
mod tests;
