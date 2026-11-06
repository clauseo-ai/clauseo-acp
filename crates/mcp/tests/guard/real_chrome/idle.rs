//! An idle Chrome contacts no host of its own: past the page the test opens, every request the
//! session's proxy sees is for `page.test` (decision record 0029, "No background hosts"). With
//! agent-browser's flags alone, Chrome for Testing 147 reached six Google hosts within 3 s of
//! starting (G17 item 5 on iris), so [`IDLE`] is well past them and keeps the test short. The
//! same holds for a Chrome restarted on the run's kept profile after the idle stop (lazy start,
//! `src/browser_guard/session.rs`). They run in nextest's `real-chrome` group, one at a time,
//! by their names.

use std::time::Duration;

use serde_json::json;

use super::{Session, binaries, call, command, handshake};
use crate::page_proxy::{HEADING, PageProxy};
use crate::support::{Guard, Scratch};

/// How long Chrome sits idle on the page.
const IDLE: Duration = Duration::from_secs(10);

#[test]
fn an_idle_chrome_contacts_no_host_but_the_page() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_chrome_idle");
    let proxy = PageProxy::start();
    let mut guard = Guard::start(command(&scratch, &binaries, &proxy));
    handshake(&mut guard);
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
    std::thread::sleep(IDLE);
    call(&mut guard, 5, "agent_browser_close", &json!({}));
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    let log = proxy.log();
    eprintln!("proxy: {log:?}");
    let background: Vec<&String> = log
        .iter()
        .filter(|request| !request.starts_with("GET http://page.test/"))
        .collect();
    assert!(background.is_empty(), "background hosts: {background:?}");
    session.assert_gone(&scratch);
}

/// The idle stop's time in this test: past [`IDLE`], so the restarted Chrome sits idle for all
/// of [`IDLE`] before the guard would stop it again.
const STOP_AFTER: Duration = Duration::from_secs(15);

/// Background hosts in the proxy's log from entry `from` on: anything but the test's page.
fn background(proxy: &PageProxy, from: usize) -> Vec<String> {
    proxy
        .log()
        .into_iter()
        .skip(from)
        .filter(|request| !request.starts_with("GET http://page.test/"))
        .collect()
}

/// After the idle stop the next call starts Chrome again on the run's kept profile, which
/// Chrome's first run has rewritten; that Chrome too contacts no host but the page while it sits
/// idle for [`IDLE`], and its profile still has `quiet`'s preferences.
#[test]
fn a_chrome_restarted_on_the_kept_profile_contacts_no_host_but_the_page() {
    let Some(binaries) = binaries() else { return };
    let scratch = Scratch::new("real_chrome_idle_restart");
    let proxy = PageProxy::start();
    let mut command = command(&scratch, &binaries, &proxy);
    command.env(
        "CLAUSEO_TEST_GUARD_IDLE_MS",
        STOP_AFTER.as_millis().to_string(),
    );
    let mut guard = Guard::start(command);
    handshake(&mut guard);
    let open = json!({"url": "http://page.test/"});
    call(&mut guard, 3, "agent_browser_open", &open);
    let first = Session::find(&scratch);
    let profiles = crate::support::profiles(&scratch.cwd());
    assert_eq!(profiles.len(), 1, "{profiles:?}");
    guard.stderr_until("stopped Chrome and agent-browser, keeping the profile");
    assert!(
        background(&proxy, 0).is_empty(),
        "background hosts before the stop: {:?}",
        background(&proxy, 0)
    );
    let restarted_at = proxy.log().len();
    call(&mut guard, 4, "agent_browser_open", &open);
    let snapshot = call(&mut guard, 5, "agent_browser_snapshot", &json!({}));
    assert!(
        snapshot.to_string().contains(HEADING),
        "snapshot: {snapshot}"
    );
    let second = Session::find(&scratch);
    assert_ne!(
        second.chrome_group, first.chrome_group,
        "Chrome did not restart"
    );
    assert_eq!(
        crate::support::profiles(&scratch.cwd()),
        profiles,
        "a new profile"
    );
    std::thread::sleep(IDLE);
    let preferences = profiles[0].join("Default").join("Preferences");
    let preferences: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(preferences).unwrap()).unwrap();
    assert_eq!(
        preferences["safebrowsing"]["enabled"], false,
        "{preferences}"
    );
    call(&mut guard, 6, "agent_browser_close", &json!({}));
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    eprintln!("proxy: {:?}", proxy.log());
    let after = background(&proxy, restarted_at);
    assert!(
        after.is_empty(),
        "background hosts after the restart: {after:?}"
    );
    second.assert_gone(&scratch);
}
