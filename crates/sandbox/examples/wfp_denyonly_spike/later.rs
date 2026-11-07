//! The rows after the review's re-check (2026-11-07): tier B's port check on the runner (WMI's
//! dynamic range, the excluded-port binds, the listener table), and the remote-user filter
//! (`remote.rs`): does Windows fill `ALE_REMOTE_USER_ID` for a loopback accept?

use std::path::Path;
use std::process::{Child, Command};

use clauseo_sandbox::ports::TIER_B_PORTS;
use clauseo_sandbox::windows::wfp::check_ports;

use super::probe::{Got, step};
use super::say;
use super::tokens::{self, Derive, Desktop, Token};

/// Prints one row and counts it (pass, fail, info).
fn row(tally: &mut [usize; 3], n: u32, who: &str, expect: &str, got: &str, pass: Option<bool>) {
    let (slot, verdict) = match pass {
        Some(true) => (0, "PASS"),
        Some(false) => (1, "FAIL"),
        None => (2, "INFO"),
    };
    if let Some(count) = tally.get_mut(slot) {
        *count += 1;
    }
    say(&format!(
        "ROW {n} {who} expect={expect} got={got} {verdict}"
    ));
}

/// Whether the pool user's plain token reaches `127.0.0.1:port`, in words.
fn reach(pool: &Token, user: &str, port: u16, exe: &Path, desktop: &Desktop, n: u32) -> String {
    let token = match tokens::derive(pool, Derive::Plain, user, None) {
        Ok(token) => token,
        Err(why) => return format!("skip:{why}"),
    };
    let log = exe.with_file_name("log").join(format!("later-{n}.txt"));
    let step_text = format!("connect:127.0.0.1:{port}");
    match tokens::run_as(&token, exe, &step_text, desktop, &log.display().to_string()) {
        Ok(code) => step(code, 0).map_or_else(
            || format!("nostart:{code:#x}"),
            |got| match got {
                Got::Reached => "reached".to_owned(),
                Got::NotRun => "notrun".to_owned(),
                Got::Filtered | Got::Failed => "blocked".to_owned(),
            },
        ),
        Err(why) => format!("nostart:{why}"),
    }
}

/// A copy of the spike under another file name listening on a free port of the range: the
/// child and its port.
#[expect(
    clippy::disallowed_methods,
    reason = "the throwaway spike copies its own exe to make a foreign program"
)]
fn foreign_listener(exe: &Path) -> Result<(Child, u16), String> {
    let copy = exe.with_file_name("foreign_listener.exe");
    std::fs::copy(exe, &copy).map_err(|e| format!("copy: {e}"))?;
    let port = TIER_B_PORTS
        .clone()
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .ok_or("no free range port")?;
    let child = Command::new(&copy)
        .args(["listen", &port.to_string()])
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok((child, port));
        }
        super::derived::pause_tenth();
    }
    Ok((child, port))
}

/// The rows; the pass, fail and info counts.
pub(crate) fn remote_rows(
    pool: &Token,
    user: &str,
    own: u16,
    exe: &Path,
    desktop: &Desktop,
) -> [usize; 3] {
    let mut tally = [0usize; 3];
    let t = &mut tally;
    let alone = check_ports(true);
    let got = format!("{alone:?}");
    row(
        t,
        14,
        "check_ports(only-the-spike's-listeners)",
        "ok",
        &got,
        Some(alone.is_ok()),
    );
    let (mut child, port) = match foreign_listener(exe) {
        Ok(started) => started,
        Err(why) => {
            row(t, 15, "foreign-listener", "started", &why, Some(false));
            return tally;
        }
    };
    let with = check_ports(true);
    let refused = with
        .as_ref()
        .err()
        .is_some_and(|gap| gap.to_string().contains("foreign_listener.exe"));
    row(
        t,
        15,
        "check_ports(a-foreign-listener-in-range)",
        "refused",
        &format!("{with:?}"),
        Some(refused),
    );
    let before = reach(pool, user, port, exe, desktop, 16);
    row(
        t,
        16,
        "pool->foreign-listener(no-remote-user-filter)",
        "reached",
        &before,
        Some(before == "reached"),
    );
    match super::remote::block_remote_user(user, exe) {
        Ok(session) => {
            let foreign = reach(pool, user, port, exe, desktop, 17);
            let filled = foreign == "blocked";
            row(
                t,
                17,
                "pool->foreign-listener(remote-user-filter)",
                "blocked",
                &foreign,
                Some(filled),
            );
            let ours = reach(pool, user, own, exe, desktop, 18);
            row(
                t,
                18,
                "pool->spike's-own-listener(remote-user-filter)",
                "reached",
                &ours,
                Some(ours == "reached"),
            );
            say(if filled {
                "REMOTE_USER filled for loopback accepts"
            } else {
                "REMOTE_USER not filled for loopback accepts (or the filter didn't match)"
            });
            drop(session);
        }
        Err(why) => row(t, 17, "remote-user-filter", "added", &why, Some(false)),
    }
    drop(child.kill());
    drop(child.wait());
    tally
}
