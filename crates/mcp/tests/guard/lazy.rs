//! Lazy start, the idle stop and the crash restart, with the stubs: no Chrome until a tool call
//! needs one; after the idle stop (shortened through the `test-support` knob) Chrome and the
//! daemon are gone, the profile stays and the next call starts both again; Chrome's first death
//! answers the call in flight and the next call restarts it, its second ends the guard; and a
//! SIGKILL of the guard while stopped or mid-restart leaves nothing. Waits poll as
//! `lifecycle.rs`'s do.

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use serde_json::{Value, json};

#[cfg(unix)]
use crate::lifecycle::{
    alive, assert_nothing_left, no_args, pid_in, session_pids, sigkill, within,
};
#[cfg(unix)]
use crate::support::{Guard, Scratch, example, guard_command, profiles, run_files};

/// The test-only variable that shortens the guard's idle stop (`src/browser_guard/session.rs`).
#[cfg(unix)]
const IDLE_KNOB: &str = "CLAUSEO_TEST_GUARD_IDLE_MS";

/// What the guard prints when the browser idles out.
#[cfg(unix)]
const IDLE_NOTE: &str = "stopped Chrome and agent-browser, keeping the profile";

/// A guard in front of `daemon_stub`, with the idle stop at `idle_ms` when given.
#[cfg(unix)]
fn guard(scratch: &Scratch, idle_ms: Option<u64>) -> Guard {
    let mut command = guard_command(scratch, &example("daemon_stub"), &no_args());
    if let Some(ms) = idle_ms {
        command.env(IDLE_KNOB, ms.to_string());
    }
    Guard::start(command)
}

/// The profile the guard's last Chrome was given (`chrome_stub` records it).
#[cfg(unix)]
fn profile_of(scratch: &Scratch, guard: &Guard) -> PathBuf {
    let file = scratch.path().join(format!("profile-of-{}", guard.pid()));
    PathBuf::from(std::fs::read_to_string(file).unwrap())
}

/// A call `daemon_stub` never answers: in flight until the guard answers it.
#[cfg(unix)]
fn held_call(id: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": "stub_hold", "arguments": {}}})
}

/// The handshake and `tools/list` start nothing: no Chrome, no profile, no daemon, and on
/// Linux no process but the guard and its reaper. The first call starts Chrome and the daemon.
#[cfg(unix)]
#[test]
fn chrome_starts_at_the_first_call_not_at_the_handshake() {
    let scratch = Scratch::new("lazy_start");
    let mut guard = guard(&scratch, None);
    for (id, method) in [(0, "initialize"), (1, "tools/list"), (2, "ping")] {
        guard.send(&json!({"jsonrpc": "2.0", "id": id, "method": method}));
        assert_eq!(guard.read()["id"], id);
    }
    guard.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    guard.send(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}));
    assert_eq!(guard.read()["id"], 3, "the notification got no answer");
    assert!(
        !scratch.path().join("chrome.pid").exists(),
        "Chrome started"
    );
    assert!(profiles(&scratch.cwd()).is_empty(), "a profile was made");
    assert!(
        crate::support::daemon_pid_file(&scratch.socket_dir()).is_none(),
        "a daemon started"
    );
    #[cfg(target_os = "linux")]
    {
        let running = crate::support::left_in(scratch.path());
        assert_eq!(
            running.len(),
            2,
            "only the guard and its reaper: {running:?}"
        );
    }
    guard.call("open");
    let pids = session_pids(&scratch);
    assert!(alive(pids[1]), "no Chrome after the first call");
    assert_eq!(profiles(&scratch.cwd()).len(), 1);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, pids);
}

/// After the idle stop Chrome and the daemon are gone and the profile stays; the next call
/// starts both again with the same profile; the end of stdin then removes everything.
#[cfg(unix)]
#[test]
fn the_idle_stop_ends_chrome_and_the_daemon_and_the_next_call_restarts() {
    let scratch = Scratch::new("lazy_idle");
    let mut guard = guard(&scratch, Some(1_000));
    guard.call("open");
    let first = session_pids(&scratch);
    let profile = profile_of(&scratch, &guard);
    write_marker(&profile);
    guard.stderr_until(IDLE_NOTE);
    assert!(within(|| !alive(first[1])), "Chrome {} is left", first[1]);
    assert!(
        within(|| !alive(first[0])),
        "the daemon {} is left",
        first[0]
    );
    assert!(
        profile.join("marker").is_file(),
        "the profile went at the idle stop"
    );
    guard.call("again");
    let mut pids = [0, 0];
    assert!(within(|| {
        pids = session_pids(&scratch);
        pids[1] != first[1] && alive(pids[1])
    }));
    assert_eq!(profile_of(&scratch, &guard), profile, "a new profile");
    assert!(
        profile.join("marker").is_file(),
        "the restart wiped the profile"
    );
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, pids);
}

#[cfg(unix)]
fn write_marker(profile: &std::path::Path) {
    crate::write::write(profile.join("marker"), "kept").unwrap();
}

/// Chrome's first death: the call in flight gets an error, the daemon is ended, the guard stays
/// and the next call starts Chrome again. Its second: the call in flight gets an error and the
/// guard exits 1, leaving nothing.
#[cfg(unix)]
#[test]
fn chrome_restarts_once_and_its_second_death_ends_the_guard() {
    let scratch = Scratch::new("lazy_crash");
    let mut guard = guard(&scratch, None);
    guard.call("open");
    let first = session_pids(&scratch);
    guard.send(&held_call("held-1"));
    sigkill(first[1]);
    let answer = guard.read();
    assert_eq!(answer["id"], "held-1", "{answer}");
    assert_eq!(answer["error"]["code"], -32603, "{answer}");
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("Chrome exited"), "{answer}");
    assert!(within(|| !alive(first[0])), "the first daemon is left");
    guard.call("again");
    let mut pids = [0, 0];
    assert!(within(|| {
        pids = session_pids(&scratch);
        pids[1] != first[1] && alive(pids[1])
    }));
    guard.send(&held_call("held-2"));
    sigkill(pids[1]);
    let answer = guard.read();
    assert_eq!(answer["id"], "held-2", "{answer}");
    assert_eq!(answer["error"]["code"], -32603, "{answer}");
    assert_eq!(guard.wait().code(), Some(1));
    let stderr = guard.stderr_text();
    assert!(
        stderr.contains("starts both again at the next browser call"),
        "{stderr}"
    );
    assert!(
        stderr.contains("clauseo-mcp browser-guard: Chrome exited; the guard ended agent-browser"),
        "{stderr}"
    );
    assert_nothing_left(&scratch, pids);
}

/// A SIGKILL of the guard once the browser has idled out: the reaper deletes the profile and
/// the lock file, and nothing of the session runs.
#[cfg(unix)]
#[test]
fn a_guard_killed_while_stopped_leaves_nothing() {
    let scratch = Scratch::new("lazy_kill_idle");
    let mut guard = guard(&scratch, Some(500));
    guard.call("open");
    let pids = session_pids(&scratch);
    guard.stderr_until(IDLE_NOTE);
    assert_eq!(
        profiles(&scratch.cwd()).len(),
        1,
        "the profile stays while stopped"
    );
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert_nothing_left(&scratch, pids);
    #[cfg(target_os = "linux")]
    {
        let left = || crate::support::left_in(scratch.path());
        assert!(within(|| left().is_empty()), "left behind: {:?}", left());
    }
    drop(guard);
}

/// A SIGKILL of the guard during a lazy restart, while the new Chrome has not listened yet: the
/// reaper, told the new Chrome's pid before the wait, ends it.
#[cfg(unix)]
#[test]
fn a_guard_killed_during_a_lazy_restart_leaves_nothing() {
    let scratch = Scratch::new("lazy_kill_restart");
    let mut guard = guard(&scratch, Some(500));
    guard.call("open");
    let first = session_pids(&scratch);
    guard.stderr_until(IDLE_NOTE);
    assert!(within(|| !alive(first[1])), "the first Chrome is left");
    scratch.chrome_mode("silent");
    guard.send_call("again");
    guard.stderr_until("Chrome started; waiting for it to listen");
    let mut chrome = 0;
    assert!(within(|| {
        chrome = pid_in(&scratch.path().join("chrome.pid"));
        chrome != first[1]
    }));
    assert!(alive(chrome));
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert!(
        within(|| !alive(chrome)),
        "the restarting Chrome {chrome} is left"
    );
    let left = || run_files(&scratch.cwd());
    assert!(within(|| left().is_empty()), "left: {:?}", left());
    #[cfg(target_os = "linux")]
    {
        let left = || crate::support::left_in(scratch.path());
        assert!(within(|| left().is_empty()), "left behind: {:?}", left());
    }
    drop(guard);
}

/// The first call waits for a claimed debugging port another process still holds (a dying run's
/// Chrome after an MCP reconnect), and starts once it is free, rather than the guard exiting.
#[cfg(unix)]
#[test]
fn the_first_call_waits_for_a_busy_debugging_port() {
    use std::net::{Ipv4Addr, TcpListener};
    let scratch = Scratch::new("lazy_port_wait");
    // A port of its own, so the other tests' starts never wait on this one.
    let port = "61151";
    let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, 61_151)).unwrap();
    let mut args = crate::support::guard_args(&scratch, &example("daemon_stub"), &no_args());
    let at = args.iter().position(|arg| arg == "--cdp-port").unwrap();
    args[at + 1] = port.into();
    let mut command = std::process::Command::new(crate::support::GUARD);
    command.args(args).envs(crate::support::host_env(&scratch));
    let mut guard = Guard::start(command);
    guard.send_call("open");
    guard.stderr_until("is taken; waiting up to 15 s for it");
    drop(holder);
    let answer = guard.read();
    assert_eq!(answer["id"], "open", "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    let pids = session_pids(&scratch);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, pids);
}

/// The run's Chrome record through the host's API: none before the first call, the running
/// Chrome after it, none at the idle stop, the new Chrome after the restart, and no record (the
/// run settled) once the guard has exited.
#[cfg(unix)]
#[test]
fn the_chrome_record_follows_each_start_and_stop() {
    use clauseo_mcp::browser_guard::{ChromeState, chrome_groups, runs_settled};
    let scratch = Scratch::new("lazy_record");
    let cwd = scratch.cwd();
    let mut guard = guard(&scratch, Some(1_000));
    guard.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    guard.read();
    assert_eq!(chrome_groups(&cwd), [ChromeState::None]);
    assert!(!runs_settled(&cwd));
    guard.call("open");
    let first = session_pids(&scratch)[1];
    let [ChromeState::Running(process)] = chrome_groups(&cwd)[..] else {
        panic!("{:?}", chrome_groups(&cwd));
    };
    assert_eq!(i32::try_from(process.id).unwrap(), first);
    assert!(!process.gone(), "a live Chrome read as gone");
    guard.stderr_until(IDLE_NOTE);
    assert_eq!(chrome_groups(&cwd), [ChromeState::None]);
    assert!(process.gone(), "the stopped Chrome read as alive");
    assert!(
        !runs_settled(&cwd),
        "a stopped run settled before the guard exits"
    );
    guard.call("again");
    let mut second = 0;
    assert!(within(|| {
        second = session_pids(&scratch)[1];
        second != first
    }));
    let [ChromeState::Running(process)] = chrome_groups(&cwd)[..] else {
        panic!("{:?}", chrome_groups(&cwd));
    };
    assert_eq!(i32::try_from(process.id).unwrap(), second);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert!(within(|| runs_settled(&cwd)), "the run never settled");
    assert_eq!(chrome_groups(&cwd), []);
    assert!(process.gone());
}

/// A killed guard's reaper ends Chrome, then deletes the record before the run's lock goes.
#[cfg(unix)]
#[test]
fn the_reaper_clears_a_killed_guards_chrome_record() {
    use clauseo_mcp::browser_guard::{ChromeState, chrome_groups, runs_settled};
    let scratch = Scratch::new("lazy_record_reaper");
    let cwd = scratch.cwd();
    let mut guard = guard(&scratch, None);
    guard.call("open");
    let pids = session_pids(&scratch);
    let [ChromeState::Running(process)] = chrome_groups(&cwd)[..] else {
        panic!("{:?}", chrome_groups(&cwd));
    };
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert!(within(|| runs_settled(&cwd)), "the run never settled");
    assert_eq!(chrome_groups(&cwd), [], "the record was left");
    assert!(process.gone());
    assert_nothing_left(&scratch, pids);
    drop(guard);
}

/// Chrome's second death while the agent floods tool calls: once the guard's final stop has run,
/// no call starts Chrome or a one-shot agent-browser again (each gets an error), so once the
/// guard has exited and its reaper is done, nothing of the session runs. Before the fix a call
/// after the final stop started a Chrome nothing killed (5 of 10 runs).
#[cfg(target_os = "linux")]
#[test]
fn calls_after_the_final_stop_start_nothing() {
    let scratch = Scratch::new("lazy_flood");
    let mut guard = guard(&scratch, None);
    guard.call("open");
    let first = session_pids(&scratch);
    sigkill(first[1]);
    guard.stderr_until("starts both again at the next browser call");
    guard.call("again");
    let mut pids = [0, 0];
    assert!(within(|| {
        pids = session_pids(&scratch);
        pids[1] != first[1] && alive(pids[1])
    }));
    sigkill(pids[1]);
    // Tool calls, which would start Chrome, and `tools/list`, which would start a one-shot.
    let status = guard.flood(|n| {
        if n % 2 == 0 {
            crate::support::open_call(&format!("flood-{n}"))
        } else {
            json!({"jsonrpc": "2.0", "id": n, "method": "tools/list"})
        }
    });
    assert_eq!(status.code(), Some(1));
    let left = || session_processes(&scratch);
    if !within(|| left().is_empty()) {
        let left = left();
        for (pid, _) in &left {
            sigkill(*pid);
        }
        panic!("left behind: {left:?}");
    }
}

/// Linux: each running process (zombies not counted) whose command line or environment names the
/// scratch folder or its socket folder: the guard's Chrome stand-in, agent-browser's stub (the
/// session's or a one-shot) and its daemon, the guard and its reaper.
#[cfg(target_os = "linux")]
fn session_processes(scratch: &Scratch) -> Vec<(i32, String)> {
    let needles = [scratch.path().to_owned(), scratch.socket_dir()]
        .map(|path| path.into_os_string().into_encoded_bytes());
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| alive(*pid))
        .filter_map(|pid| {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
            let names = |text: &[u8]| {
                needles.iter().any(|needle| {
                    text.windows(needle.len())
                        .any(|window| window == needle.as_slice())
                })
            };
            (names(&cmdline) || names(&environ))
                .then(|| (pid, String::from_utf8_lossy(&cmdline).replace('\0', " ")))
        })
        .collect()
}
