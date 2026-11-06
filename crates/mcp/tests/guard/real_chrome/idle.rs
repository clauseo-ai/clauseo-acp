//! An idle Chrome contacts no host of its own: past the page the test opens, every request the
//! session's proxy sees is for `page.test` (decision record 0029, "No background hosts"). With
//! agent-browser's flags alone, Chrome for Testing 147 reached six Google hosts within 3 s of
//! starting (G17 item 5 on iris), so [`IDLE`] is well past them and keeps the test short. It
//! runs in nextest's `real-chrome` group, one at a time, by its name.

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
