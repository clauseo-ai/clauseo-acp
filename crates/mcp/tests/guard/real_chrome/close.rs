//! `agent_browser_close` with the real agent-browser v0.41.1 and a real Chrome: its reply stops
//! the guard's Chrome and agent-browser, keeping the profile, so Chrome's debugging port lives
//! only from the first browser call to the close; the next call starts both again, and a close
//! while they are stopped starts nothing and gets agent-browser's own result from the guard.

use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

use super::session::Session;
use super::{binaries, call, command, handshake, processes};
use crate::page_proxy::{HEADING, PageProxy};
use crate::support::{Guard, Scratch};

/// The processes running with the scratch folder's profile flag, and the debugging port the
/// first of them was given (`--remote-debugging-port=`).
fn chrome_runs(scratch: &Scratch) -> (bool, Option<u16>) {
    let flag = format!(
        "--user-data-dir={}",
        scratch.cwd().join("chrome-profile-").display()
    );
    let mut port = None;
    let mut runs = false;
    for pid in processes() {
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<&[u8]> = cmd.split(|b| *b == 0).collect();
        if args.iter().any(|arg| arg.starts_with(flag.as_bytes())) {
            runs = true;
            port = port.or_else(|| {
                args.iter().find_map(|arg| {
                    std::str::from_utf8(arg.strip_prefix(b"--remote-debugging-port=")?)
                        .ok()?
                        .parse()
                        .ok()
                })
            });
        }
    }
    (runs, port)
}

/// Whether `ready` holds within 10 s, looking every 50 ms.
fn within(mut ready: impl FnMut() -> bool) -> bool {
    (0..200).any(|_| {
        let now = ready();
        if !now {
            std::thread::sleep(Duration::from_millis(50));
        }
        now
    })
}

/// Whether something accepts connections on 127.0.0.1:`port`.
fn listens(port: u16) -> bool {
    TcpStream::connect_timeout(
        &(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// What of `session` runs but the guard and its reaper.
fn browser_left(session: &Session) -> Vec<String> {
    session
        .left()
        .into_iter()
        .filter(|process| {
            !process.contains("clauseo-mcp browser-guard")
                && !process.contains("clauseo-mcp browser-reaper")
        })
        .collect()
}

/// Open (cold), open again (warm), close: within a few seconds no Chrome or daemon runs, the
/// first Chrome's debugging port takes no connection and the profile is still there. The next
/// open starts Chrome again (cold) on the kept profile and the page reads; the guard's exit then
/// leaves nothing. The `agent_browser_open` times it prints are the cold start the close adds.
#[test]
fn close_stops_chrome_keeps_the_profile_and_the_next_call_starts_it() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_close");
    let proxy = PageProxy::start();
    let mut guard = Guard::start(command(&scratch, &binaries, &proxy));
    handshake(&mut guard);
    let open = json!({"url": "http://page.test/"});
    eprint!("cold ");
    call(&mut guard, 3, "agent_browser_open", &open);
    eprint!("warm ");
    call(&mut guard, 4, "agent_browser_open", &open);
    let first = Session::find(&scratch);
    let port = chrome_runs(&scratch).1.expect("Chrome's debugging port");
    assert!(listens(port), "Chrome does not listen on {port}");
    call(&mut guard, 5, "agent_browser_close", &json!({}));
    guard.stderr_until("agent_browser_close: stopped Chrome and agent-browser");
    assert!(
        within(|| browser_left(&first).is_empty()),
        "left after the close: {:?}",
        browser_left(&first)
    );
    assert!(!chrome_runs(&scratch).0, "Chrome runs after the close");
    assert!(
        !listens(port),
        "the first Chrome's port {port} still listens"
    );
    assert_eq!(
        crate::support::profiles(&scratch.cwd()).len(),
        1,
        "the close deleted the profile"
    );
    eprint!("after the close ");
    call(&mut guard, 6, "agent_browser_open", &open);
    let snapshot = call(&mut guard, 7, "agent_browser_snapshot", &json!({}));
    assert!(
        snapshot.to_string().contains(HEADING),
        "snapshot: {snapshot}"
    );
    let second = Session::find(&scratch);
    assert_ne!(second.chrome_group, first.chrome_group);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    second.assert_gone(&scratch);
}

/// A close while the browser is stopped (before any call, and after a close's stop) starts no
/// Chrome, and the guard's answer is the result the real agent-browser gave the close it relayed:
/// a bump that changes agent-browser's result fails here (`relay.rs`'s `CLOSED_RESULT`).
#[test]
fn a_close_while_stopped_starts_nothing_and_gets_agent_browsers_result() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_close_stopped");
    let proxy = PageProxy::start();
    let mut guard = Guard::start(command(&scratch, &binaries, &proxy));
    handshake(&mut guard);
    let before = call(&mut guard, 3, "agent_browser_close", &json!({}));
    assert!(
        !chrome_runs(&scratch).0,
        "a close before any call started Chrome"
    );
    assert!(
        crate::support::profiles(&scratch.cwd()).is_empty(),
        "a close before any call made a profile"
    );
    call(
        &mut guard,
        4,
        "agent_browser_open",
        &json!({"url": "http://page.test/"}),
    );
    let session = Session::find(&scratch);
    let relayed: Value = call(&mut guard, 5, "agent_browser_close", &json!({}));
    guard.stderr_until("agent_browser_close: stopped Chrome and agent-browser");
    assert!(
        within(|| browser_left(&session).is_empty()),
        "left after the close: {:?}",
        browser_left(&session)
    );
    let after = call(&mut guard, 6, "agent_browser_close", &json!({}));
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !chrome_runs(&scratch).0,
        "a close while stopped started Chrome"
    );
    eprintln!("agent-browser's close result: {relayed}");
    assert_eq!(
        before, relayed,
        "the guard's close result is not agent-browser's"
    );
    assert_eq!(
        after, relayed,
        "the guard's close result is not agent-browser's"
    );
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    session.assert_gone(&scratch);
}
