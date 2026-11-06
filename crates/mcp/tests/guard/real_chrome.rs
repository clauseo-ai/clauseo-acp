//! The regression test for S27: the real `clauseo-mcp browser-guard` in front of the real
//! agent-browser v0.41.1 and a real Chrome. Before the guard started Chrome itself, every
//! `agent_browser_open` timed out after 30 s (decision record 0029, "Chrome on the claimed
//! port"). Lazy start with the real binaries is in `real_lazy.rs` (`real_chrome::lazy`), on
//! these helpers.
//!
//! Inputs: `CLAUSEO_TEST_AGENT_BROWSER`, the agent-browser binary (CI's Linux job downloads the
//! pinned one); `CLAUSEO_TEST_CHROME`, a Chrome, else `/usr/bin/google-chrome` or
//! `/usr/bin/chromium`. Without agent-browser or a Chrome the tests say so on stderr and pass;
//! with `CLAUSEO_TEST_AGENT_BROWSER` set, a missing Chrome fails them. With `CI` set Chrome runs
//! with `--no-sandbox` (a wrapper script adds it): CI runs the tests under `no_new_privs`,
//! where Chrome's setuid sandbox cannot start.
//!
//! Linux only: the process checks read `/proc`.

#![cfg(target_os = "linux")]
#![expect(
    clippy::disallowed_methods,
    reason = "the real-Chrome test times its steps and waits for Chrome's downloads and exit"
)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::page_proxy::{DOWNLOADS, HEADING, PageProxy};
use crate::support::{CDP_PORT, GUARD, Guard, Scratch, guard_args, host_env, left_in};
use crate::write;

#[path = "real_chrome/idle.rs"]
mod idle;
// Lazy start with the real binaries, under `real_chrome::` so it joins nextest's `real-chrome`
// test group (`.config/nextest.toml`).
#[path = "real_lazy.rs"]
mod lazy;

/// The binaries, or `None` (with a note) when the test cannot run here.
pub(crate) fn binaries() -> Option<(PathBuf, PathBuf)> {
    let Some(agent_browser) = std::env::var_os("CLAUSEO_TEST_AGENT_BROWSER") else {
        eprintln!(
            "SKIPPED: set CLAUSEO_TEST_AGENT_BROWSER to agent-browser v0.41.1 (and \
             CLAUSEO_TEST_CHROME to a Chrome) to run the real-Chrome guard test"
        );
        return None;
    };
    let chrome = std::env::var_os("CLAUSEO_TEST_CHROME")
        .map(PathBuf::from)
        .or_else(|| {
            ["/usr/bin/google-chrome", "/usr/bin/chromium"]
                .map(PathBuf::from)
                .into_iter()
                .find(|path| path.is_file())
        })
        .expect("CLAUSEO_TEST_AGENT_BROWSER is set but no Chrome was found");
    Some((PathBuf::from(agent_browser), chrome))
}

/// The Chrome the guard starts: `chrome` itself, or a script that runs it, adding
/// `--no-sandbox` under `CI`, and with `quiet` sending its stderr to `/dev/null` (so the guard
/// never sees the endpoint and stays in its wait for it).
fn chrome_for(scratch: &Scratch, chrome: &Path, quiet: bool) -> PathBuf {
    let ci = std::env::var_os("CI").is_some();
    if !ci && !quiet {
        return chrome.to_owned();
    }
    let wrapper = scratch.path().join("chrome-wrapper.sh");
    let quoted = chrome.to_str().unwrap().replace('\'', r"'\''");
    let sandbox = if ci { " --no-sandbox" } else { "" };
    // Chrome keeps the guard's stderr pipe open on fd 3, so the guard sees no end, only no line.
    let stderr = if quiet { " 3>&2 2>/dev/null" } else { "" };
    write::write(
        &wrapper,
        format!("#!/bin/sh\nexec '{quoted}'{sandbox} \"$@\"{stderr}\n"),
    )
    .unwrap();
    write::set_mode(&wrapper, 0o755).unwrap();
    wrapper
}

/// A loopback port free now, standing in for one the host claimed: the tests run at once, so
/// each needs its own debugging and stream ports.
fn free_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port().to_string()
}

/// The guard as the host starts it, or under `parent_stub`, through the test's proxy.
pub(crate) fn command(
    scratch: &Scratch,
    binaries: &(PathBuf, PathBuf),
    proxy: &PageProxy,
) -> Command {
    command_with(scratch, binaries, proxy, false).0
}

/// [`command`], with Chrome's stderr hidden from the guard if `quiet`; and its debugging port.
fn command_with(
    scratch: &Scratch,
    binaries: &(PathBuf, PathBuf),
    proxy: &PageProxy,
    quiet: bool,
) -> (Command, String) {
    let cdp = free_port();
    let command = command_on(scratch, binaries, proxy, quiet, [&cdp, &free_port()]);
    (command, cdp)
}

/// [`command_with`] on the given debugging and stream ports.
fn command_on(
    scratch: &Scratch,
    binaries: &(PathBuf, PathBuf),
    proxy: &PageProxy,
    quiet: bool,
    [cdp, stream]: [&str; 2],
) -> Command {
    let mcp = ["mcp", "--tools", "core"].map(OsStr::new);
    let args = guard_args(scratch, &binaries.0, &mcp)
        .into_iter()
        .map(|arg| if arg == CDP_PORT { cdp.into() } else { arg });
    let mut command = Command::new(GUARD);
    command
        .args(args)
        .envs(host_env(scratch))
        .env("AGENT_BROWSER_STREAM_PORT", stream)
        .env(
            "AGENT_BROWSER_PROXY",
            format!("http://127.0.0.1:{}", proxy.port),
        )
        .env(
            "AGENT_BROWSER_EXECUTABLE_PATH",
            chrome_for(scratch, &binaries.1, quiet),
        );
    command
}

/// Sends a request and returns its result, skipping notifications; fails on an error or a tool
/// error.
pub(crate) fn request(guard: &mut Guard, id: u64, method: &str, params: &Value) -> Value {
    guard.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    loop {
        let reply = guard.read();
        if reply["id"] != json!(id) {
            continue;
        }
        assert!(reply.get("error").is_none(), "{method}: {reply}");
        let result = reply["result"].clone();
        assert_ne!(result["isError"], json!(true), "{method}: {result}");
        return result;
    }
}

pub(crate) fn call(guard: &mut Guard, id: u64, tool: &str, arguments: &Value) -> Value {
    let started = Instant::now();
    let result = request(
        guard,
        id,
        "tools/call",
        &json!({"name": tool, "arguments": arguments}),
    );
    eprintln!("{tool}: {:.2} s", started.elapsed().as_secs_f64());
    result
}

/// `initialize` and `tools/list`; the tools the test calls are listed.
pub(crate) fn handshake(guard: &mut Guard) {
    let init = json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {},
        "clientInfo": {"name": "clauseo-guard-regression", "version": "0"},
    });
    let result = request(guard, 1, "initialize", &init);
    assert_eq!(result["serverInfo"]["name"], "agent-browser", "{result}");
    guard.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let tools = request(guard, 2, "tools/list", &json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for tool in [
        "agent_browser_open",
        "agent_browser_snapshot",
        "agent_browser_click",
        "agent_browser_close",
    ] {
        assert!(names.contains(&tool), "no {tool} in {names:?}");
    }
}

/// The session's processes: Chrome's process group (its leader found by its profile in
/// `/proc/*/cmdline`), agent-browser's daemon (its pid file, if it started), and any process
/// whose command line names the scratch folder (Chrome's crashpad handlers lead groups of their
/// own).
pub(crate) struct Session {
    pub(crate) chrome_group: i32,
    daemon: Option<i32>,
    scratch: PathBuf,
}

impl Session {
    pub(crate) fn find(scratch: &Scratch) -> Self {
        let flag = format!(
            "--user-data-dir={}",
            scratch.cwd().join("chrome-profile-").display()
        );
        let chrome = processes()
            .into_iter()
            .find(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmd| {
                    cmd.split(|b| *b == 0)
                        .any(|arg| arg.starts_with(flag.as_bytes()))
                })
            })
            .expect("Chrome with the session's profile");
        let daemon = crate::support::daemon_pid_file(&scratch.socket_dir())
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| text.trim().parse().ok());
        Self {
            chrome_group: group_of(chrome).unwrap(),
            daemon,
            scratch: scratch.path().to_owned(),
        }
    }

    /// What of the session still runs (zombies not counted).
    pub(crate) fn left(&self) -> Vec<String> {
        processes()
            .into_iter()
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let (_, rest) = stat.rsplit_once(") ")?;
                let fields: Vec<&str> = rest.split(' ').collect();
                let zombie = fields.first() == Some(&"Z");
                let in_group =
                    fields.get(2).and_then(|g| g.parse().ok()) == Some(self.chrome_group);
                (!zombie && (in_group || Some(pid) == self.daemon)).then(|| stat.clone())
            })
            .chain(left_in(&self.scratch))
            .collect()
    }

    pub(crate) fn assert_gone(&self, scratch: &Scratch) {
        let gone = (0..200).any(|_| {
            let none = self.left().is_empty();
            if !none {
                std::thread::sleep(Duration::from_millis(50));
            }
            none
        });
        assert!(gone, "left behind: {:?}", self.left());
        assert_eq!(
            crate::support::run_files(&scratch.cwd()),
            Vec::<PathBuf>::new()
        );
    }
}

pub(crate) fn processes() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect()
}

fn group_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(2)?.parse().ok()
}

fn listed(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Opens the page through the guard, checks its snapshot, clicks the four downloads (each a
/// protected name) and closes; then the guard's exit leaves no process of the session.
#[test]
fn a_page_opens_through_the_guard_with_real_chrome() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_chrome");
    let proxy = PageProxy::start();
    let started = Instant::now();
    let mut guard = Guard::start(command(&scratch, &binaries, &proxy));
    handshake(&mut guard);
    eprintln!(
        "guard and Chrome up: {:.2} s",
        started.elapsed().as_secs_f64()
    );
    call(
        &mut guard,
        3,
        "agent_browser_open",
        &json!({"url": "http://page.test/"}),
    );
    let snapshot = call(&mut guard, 4, "agent_browser_snapshot", &json!({}));
    assert!(
        snapshot.to_string().contains(HEADING),
        "snapshot: {snapshot}"
    );
    let session = Session::find(&scratch);

    for (n, _) in DOWNLOADS.iter().enumerate() {
        let selector = format!("#d{n}");
        call(
            &mut guard,
            10 + n as u64,
            "agent_browser_click",
            &json!({"selector": selector}),
        );
    }
    // Every download arrives, and none under a protected name, in any case. Chrome may already
    // have made one unique (`claude (1).MD` beside `CLAUDE.md`), so names are checked by rule
    // and contents by the name each link sent.
    let downloads = scratch.cwd().join("downloads");
    let sent: BTreeSet<String> = DOWNLOADS
        .iter()
        .map(|name| format!("planted by a web page as {name}\n"))
        .collect();
    let contents = || -> BTreeSet<String> {
        listed(&downloads)
            .iter()
            .filter_map(|name| std::fs::read_to_string(downloads.join(name)).ok())
            .collect()
    };
    let arrived = (0..200).any(|_| {
        let done = contents() == sent;
        if !done {
            std::thread::sleep(Duration::from_millis(50));
        }
        done
    });
    let names = listed(&downloads);
    eprintln!("downloads: {names:?}");
    assert!(arrived, "downloads: {names:?}; proxy: {:?}", proxy.log());
    let protected = [".mcp.json", "claude.md", "agents.md"];
    for name in &names {
        let folded = name.to_ascii_lowercase();
        assert!(!protected.contains(&folded.as_str()), "{name} in {names:?}");
    }
    assert!(names.contains("CLAUDE (download).md"), "{names:?}");
    assert!(names.contains("AGENTS (download).md"), "{names:?}");

    call(&mut guard, 20, "agent_browser_close", &json!({}));
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    eprintln!("whole run: {:.2} s", started.elapsed().as_secs_f64());
    assert!(
        proxy.log().contains(&"GET http://page.test/".to_owned()),
        "{:?}",
        proxy.log()
    );
    session.assert_gone(&scratch);
}

/// The agent (`parent_stub`) is killed with SIGKILL while the test still holds the guard's stdin:
/// the guard notices its parent is gone and leaves no process of the session.
#[test]
fn a_killed_parent_leaves_no_chrome_and_no_daemon() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_parent");
    let proxy = PageProxy::start();
    let guard_command = command(&scratch, &binaries, &proxy);
    let mut parent = Command::new(crate::support::example("parent_stub"));
    parent
        .arg(guard_command.get_program())
        .args(guard_command.get_args())
        .envs(
            guard_command
                .get_envs()
                .filter_map(|(name, value)| Some((name, value?))),
        );
    let guard_pid_file = scratch.path().join("guard.pid");
    parent.env("PARENT_STUB_CHILD_PID", &guard_pid_file);
    let mut parent = Guard::start(parent);
    handshake(&mut parent);
    call(
        &mut parent,
        3,
        "agent_browser_open",
        &json!({"url": "http://page.test/"}),
    );
    let session = Session::find(&scratch);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(parent.pid()).unwrap()),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    session.assert_gone(&scratch);
    let guard: i32 = std::fs::read_to_string(&guard_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let guard_gone = (0..200).any(|_| {
        let gone = group_of(guard).is_none();
        if !gone {
            std::thread::sleep(Duration::from_millis(50));
        }
        gone
    });
    assert!(guard_gone, "the guard {guard} is left");
    drop(parent);
}

/// The guard is killed with SIGKILL while it waits for Chrome's endpoint (Chrome's stderr hidden
/// from it, so the wait lasts), once Chrome and its helpers run: nothing of Chrome is left, the
/// crashpad handlers included, and the profile is gone. Before the reaper learnt Chrome's pid at
/// its start, all of Chrome was left.
#[test]
fn a_guard_killed_while_chrome_starts_leaves_no_chrome() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_kill_start");
    let proxy = PageProxy::start();
    let (command, cdp) = command_with(&scratch, &binaries, &proxy, true);
    let mut guard = Guard::start(command);
    // The first call starts Chrome (the guard's lazy start); its answer never comes.
    guard.send(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"url": "http://page.test/"}}}));
    guard.stderr_until("Chrome started; waiting for it to listen");
    // 60 s: on the loaded CI runner Chrome once took over 10 s to listen (.config/nextest.toml).
    let listening = (0..1200).any(|_| {
        let up = std::net::TcpStream::connect(format!("127.0.0.1:{cdp}")).is_ok();
        if !up {
            std::thread::sleep(Duration::from_millis(50));
        }
        up
    });
    assert!(listening, "Chrome never listened on {cdp}");
    let session = Session::find(&scratch);
    eprintln!("running before the kill: {}", left_in(scratch.path()).len());
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(guard.pid()).unwrap()),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    session.assert_gone(&scratch);
    drop(guard);
}

/// The code owner's repro (M1) with the real agent-browser and Chrome: guard A is killed once its
/// daemon runs, and guard B starts at once on the same `--cwd` and ports, as an MCP reconnect
/// does. A's reaper ends A's run alone: B's profile and staging folder stay, and B's daemon
/// (`run-<B's pid>-<hex>.sock`) still serves B. Before, A's reaper deleted B's profile.
#[test]
fn a_killed_guards_reaper_leaves_the_next_run_alone() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_rerun");
    let proxy = PageProxy::start();
    let ports = [free_port(), free_port()];
    let ports = [ports[0].as_str(), ports[1].as_str()];
    let mut first = Guard::start(command_on(&scratch, &binaries, &proxy, false, ports));
    handshake(&mut first);
    call(
        &mut first,
        3,
        "agent_browser_open",
        &json!({"url": "http://page.test/"}),
    );
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(first.pid()).unwrap()),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    let mut next = Guard::start(command_on(&scratch, &binaries, &proxy, false, ports));
    handshake(&mut next);
    call(
        &mut next,
        3,
        "agent_browser_open",
        &json!({"url": "http://page.test/"}),
    );
    let profiles: Vec<PathBuf> = crate::support::profiles(&scratch.cwd())
        .into_iter()
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            name.starts_with(&format!("chrome-profile-{}-", next.pid()))
        })
        .collect();
    let [profile] = profiles.as_slice() else {
        panic!("not one profile of the next run: {profiles:?}");
    };
    let daemon_of_next = || -> Option<i32> {
        let prefix = format!("run-{}-", next.pid());
        std::fs::read_dir(scratch.socket_dir())
            .ok()?
            .flatten()
            .find_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_prefix(&prefix)?.strip_suffix(".pid")?;
                std::fs::read_to_string(entry.path())
                    .ok()?
                    .trim()
                    .parse()
                    .ok()
            })
    };
    let daemon = daemon_of_next().expect("the next run's daemon pid file");
    let kept = (0..60).all(|_| {
        std::thread::sleep(Duration::from_millis(50));
        profile.join("Default").join("Preferences").is_file()
            && profile.join("downloads-staging").is_dir()
    });
    assert!(kept, "the next run's profile {} went", profile.display());
    // A's reaper has ended A's daemon twice by now (at once, and about 1 s later); B's is the
    // same process still, and serves B.
    assert_eq!(daemon_of_next(), Some(daemon));
    assert!(
        group_of(daemon).is_some(),
        "the next run's daemon {daemon} was killed"
    );
    let snapshot = call(&mut next, 4, "agent_browser_snapshot", &json!({}));
    assert!(snapshot.to_string().contains(HEADING), "{snapshot}");
    let (status, _) = next.finish();
    assert_eq!(status.code(), Some(0));
    drop(first);
}
