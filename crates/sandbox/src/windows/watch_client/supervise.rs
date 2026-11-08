//! The supervisor: one attempt after another, at most once every [`RESTART_EVERY`], each
//! starting the task, polling the name, opening and checking the pipe, then reading it.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use super::route::Lines;
use super::{
    Capped, ConnectError, Down, PIPE_WAIT, POLL_EVERY, PROVIDER_FULL, Pipe, READ_WAIT,
    RESTART_EVERY, RETRIGGER_AFTER, Shared, StartError, TICK, Watch, route,
};
use crate::windows::setup::watch::protocol::is_pipe_name;

/// Connects and reads for `generation` until no launch is registered, starting again at
/// most once every [`RESTART_EVERY`]. `started` hears of the first attempt's end.
pub(super) fn supervise<W: Watch>(shared: &Shared<W>, generation: u64, started: mpsc::Sender<()>) {
    let mut started = Some(started);
    let mut last_start: Option<Duration> = None;
    loop {
        if let Some(last) = last_start
            && !wait_until(shared, generation, last + RESTART_EVERY)
        {
            return;
        }
        if !shared.current(generation) {
            return;
        }
        // A retry keeps the last status until it ends: down, and why, while it tries.
        last_start = Some(shared.watch.now());
        let attempt = connect(shared, generation);
        if !shared.current(generation) {
            return;
        }
        match attempt {
            Ok(pipe) => {
                shared.live(generation);
                tell(&mut started);
                read(shared, generation, pipe);
                if !shared.current(generation) {
                    return;
                }
                // The type has no count for "unknown": a line in the log instead.
                let line = "the denial reporter's pipe closed: refusals until it reconnects \
                            (within a minute) aren't reported";
                shared.down(generation, Down::Ended, line);
            }
            Err((down, line)) => {
                shared.down(generation, down, &line);
                tell(&mut started);
            }
        }
    }
}

/// Waits until `at`, in steps of at most a second; `false` once `generation` is stale.
fn wait_until<W: Watch>(shared: &Shared<W>, generation: u64, at: Duration) -> bool {
    loop {
        if !shared.current(generation) {
            return false;
        }
        let now = shared.watch.now();
        if now >= at {
            return true;
        }
        shared.watch.sleep(at.saturating_sub(now).min(TICK));
    }
}

/// One attempt: starts the task, polls the name, opens and checks the pipe, asking the task
/// once more after [`RETRIGGER_AFTER`].
fn connect<W: Watch>(shared: &Shared<W>, generation: u64) -> Result<W::Pipe, (Down, String)> {
    if shared.watch.turned_off() {
        let line = "the denial reporter is turned off by policy \
                    (DenialReporter is 0, or not a readable DWORD)";
        return Err((Down::TurnedOff, line.to_owned()));
    }
    request(shared)?;
    let began = shared.watch.now();
    let mut retriggered = false;
    // A name whose server was refused, or a key that was, and why: polled past, in case a
    // new run publishes another.
    let mut refused: Option<(Option<String>, String)> = None;
    loop {
        if !shared.current(generation) {
            return Err((Down::Ended, String::new()));
        }
        match shared.watch.pipe_name() {
            Ok(Some(name)) if is_pipe_name(&name) => {
                let seen = refused
                    .as_ref()
                    .is_some_and(|(old, _)| old.as_ref() == Some(&name));
                if !seen {
                    match shared.watch.connect(&name) {
                        Ok(pipe) => return Ok(pipe),
                        Err(ConnectError::Absent) => {}
                        Err(ConnectError::NotTheServer(why)) => {
                            refused = Some((Some(name), why));
                        }
                        Err(ConnectError::Failed(why)) => {
                            let line = format!("the denial reporter's pipe didn't open: {why}");
                            return Err((Down::Failed, line));
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(why) => refused = Some((None, why)),
        }
        let waited = shared.watch.now().saturating_sub(began);
        if waited >= PIPE_WAIT {
            break;
        }
        if !retriggered && waited >= RETRIGGER_AFTER {
            retriggered = true;
            request(shared)?;
        }
        shared.watch.sleep(POLL_EVERY);
    }
    if let Some((_, why)) = refused {
        let line = format!("the denial reporter's pipe was refused: {why}");
        return Err((Down::NotTheServer, line));
    }
    match shared.watch.last_exit() {
        Some(PROVIDER_FULL) => {
            let line = "the file provider's sessions are taken (the watcher exited 68)";
            Err((Down::ProviderFull, line.to_owned()))
        }
        Some(code) if code != 0 && code < 256 => {
            let line = format!("the denial reporter's watcher exited {code}");
            Err((Down::NoPipe, line))
        }
        _ => {
            let line = "the denial reporter's watcher published no pipe within 5 s";
            Err((Down::NoPipe, line.to_owned()))
        }
    }
}

/// Asks the task to run.
fn request<W: Watch>(shared: &Shared<W>) -> Result<(), (Down, String)> {
    match shared.watch.request() {
        Ok(()) => Ok(()),
        Err(StartError::NotRegistered) => {
            let line = r"the denial reporter's task \Clauseo\SandboxWatch isn't registered";
            Err((Down::NotRegistered, line.to_owned()))
        }
        Err(StartError::Failed(why)) => {
            let line = format!("the denial reporter's task didn't start: {why}");
            Err((Down::Failed, line))
        }
    }
}

/// Reads lines from `pipe` until it ends or `generation` is stale, then drops (closes) it.
fn read<W: Watch>(shared: &Shared<W>, generation: u64, mut pipe: W::Pipe) {
    let mut buf = vec![0u8; 64 * 1024];
    let mut lines = Lines::default();
    let mut next_tick = shared.watch.now() + TICK;
    loop {
        if !shared.current(generation) {
            return;
        }
        match pipe.read(&mut buf, READ_WAIT) {
            Ok(Some(0)) => return,
            Ok(Some(n)) => {
                for line in lines.push(buf.get(..n).unwrap_or_default()) {
                    if let Ok(text) = std::str::from_utf8(&line) {
                        route::deliver(shared, text);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let line = format!("reading the denial reporter's pipe failed: {error}");
                shared.watch.log(true, &line);
                return;
            }
        }
        let now = shared.watch.now();
        if now >= next_tick {
            tick_all(shared);
            next_tick = now + TICK;
        }
    }
}

/// Ticks every registered sink, outside the lock.
fn tick_all<W: Watch>(shared: &Shared<W>) {
    let sinks: Vec<Arc<dyn Capped>> = shared
        .lock()
        .launches
        .values()
        .map(|entry| Arc::clone(&entry.sink))
        .collect();
    for sink in sinks {
        sink.tick();
    }
}

/// Tells the first registration the first attempt ended, once.
fn tell(started: &mut Option<mpsc::Sender<()>>) {
    // A registration that stopped waiting has nothing to hear.
    if let Some(started) = started.take() {
        started.send(()).unwrap_or_default();
    }
}
