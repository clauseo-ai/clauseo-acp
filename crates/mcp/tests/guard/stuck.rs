//! agent-browser stuck on `--allowed-domains`, with the stubs (`daemon_stub`'s `stub-stuck` and
//! `stub-quoted` opens): the guard stops the browser once per run, keeping the profile and
//! answering what is in flight; a second refusal stops nothing and says how to get out; a page's
//! error that quotes the refusal stops nothing at all.

#[cfg(unix)]
use serde_json::{Value, json};

#[cfg(unix)]
use crate::lazy::{guard, held_call, profile_of, write_marker};
#[cfg(unix)]
use crate::lifecycle::{alive, assert_nothing_left, session_pids, within};
#[cfg(unix)]
use crate::support::Scratch;

/// What the agent gets for the refusal that stops the browser.
#[cfg(unix)]
const RESTARTED: &str = "Clauseo already controls which sites the browser may open; open the page \
                         without allowedDomains. Clauseo restarted the browser, so the next call \
                         starts clean.";
/// What it gets for any later one.
#[cfg(unix)]
const STUCK: &str = "The browser is stuck on a site restriction; close it with agent_browser_close, \
                     then open the page again without allowedDomains. Clauseo already controls \
                     which sites the browser may open.";
/// What the guard notes when it stops the browser for it.
#[cfg(unix)]
const STOP_NOTE: &str = "agent-browser refused --allowed-domains on the guard's Chrome; stopped";

/// Kills, when dropped, the process groups of the session's daemon stub and Chrome stand-in
/// that last wrote their pids, so a test that fails half way leaves neither running. Made before
/// the guard, so it drops after it: the killed guard starts nothing more.
#[cfg(unix)]
struct Reap<'a>(&'a Scratch);

#[cfg(unix)]
impl Drop for Reap<'_> {
    fn drop(&mut self) {
        use nix::sys::signal::{Signal, kill, killpg};
        use nix::unistd::Pid;
        let pid_in = |path: std::path::PathBuf| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
        };
        let daemon = crate::support::daemon_pid_file(&self.0.socket_dir()).and_then(pid_in);
        let chrome = pid_in(self.0.path().join("chrome.pid"));
        for pid in [daemon, chrome].into_iter().flatten() {
            // Gone already when the test passed.
            killpg(Pid::from_raw(pid), Signal::SIGKILL).unwrap_or_default();
            kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap_or_default();
        }
    }
}

/// An `open` the daemon stub answers with an error: `stub-stuck` or `stub-quoted`.
#[cfg(unix)]
fn open_of(id: &str, page: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": "agent_browser_open",
            "arguments": {"url": format!("https://example.com/{page}")}}})
}

/// The text of a tool result, or the message of an error.
#[cfg(unix)]
fn said(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"]
        .as_str()
        .or_else(|| reply["error"]["message"].as_str())
        .unwrap_or_default()
}

/// A page's thrown error quoting the refusal goes to the agent as agent-browser sent it, and
/// stops nothing. The real refusal stops Chrome and the daemon before its reply goes out: the
/// reply says so, the call in flight gets the same words, the profile stays, and the next call
/// starts both again on it. A second real refusal in the run stops nothing, and says to close.
#[cfg(unix)]
#[test]
fn a_stuck_session_is_stopped_once_keeping_the_profile_and_answering_what_is_in_flight() {
    let scratch = Scratch::new("stuck_once");
    let _reap = Reap(&scratch);
    let mut guard = guard(&scratch, None);
    guard.call("open");
    let first = session_pids(&scratch);
    let profile = profile_of(&scratch, &guard);
    write_marker(&profile);

    guard.send(&open_of("quoted", "stub-quoted"));
    let reply = guard.read();
    assert_eq!(reply["id"], "quoted", "{reply}");
    assert!(said(&reply).starts_with("Evaluation error: "), "{reply}");
    assert!(
        alive(first[1]) && alive(first[0]),
        "a quoted refusal stopped the browser"
    );

    guard.send(&held_call("held"));
    guard.send(&open_of("stuck-1", "stub-stuck"));
    let replies = [guard.read(), guard.read()];
    let of = |id: &str| replies.iter().find(|reply| reply["id"] == id).unwrap();
    assert_eq!(of("stuck-1")["result"]["isError"], true, "{replies:?}");
    assert_eq!(said(of("stuck-1")), RESTARTED, "{replies:?}");
    assert_eq!(said(of("held")), RESTARTED, "{replies:?}");
    guard.stderr_until(STOP_NOTE);
    assert!(within(|| !alive(first[1])), "Chrome {} is left", first[1]);
    assert!(
        within(|| !alive(first[0])),
        "the daemon {} is left",
        first[0]
    );
    assert!(
        profile.join("marker").is_file(),
        "the stop deleted the profile"
    );

    guard.call("again");
    let mut second = [0, 0];
    assert!(within(|| {
        second = session_pids(&scratch);
        second[1] != first[1] && alive(second[1])
    }));
    assert_eq!(profile_of(&scratch, &guard), profile, "a new profile");
    assert!(
        profile.join("marker").is_file(),
        "the restart wiped the profile"
    );

    guard.send(&open_of("stuck-2", "stub-stuck"));
    let reply = guard.read();
    assert_eq!(reply["id"], "stuck-2", "{reply}");
    assert_eq!(said(&reply), STUCK, "{reply}");
    assert_eq!(
        session_pids(&scratch),
        second,
        "the second refusal restarted the browser"
    );
    assert!(
        alive(second[1]) && alive(second[0]),
        "the second refusal stopped the browser"
    );

    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, second);
}
