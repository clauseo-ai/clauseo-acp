//! The `cancel` row: what the host does on a cancel, and whether it leaves a suspended runner
//! behind. The host closes the outer runner's stdin, then kills the outer 2 s later. The outer
//! gives the agent 1 s, ends the job, waits up to 400 ms for it to empty, then may start a
//! detached end sweeper (`clauseo-sandbox-run --sweep` as the pool user, created suspended, its
//! DACL shut, then resumed), or skips it when less than 500 ms of the 2 s is left
//! (`clauseo-sandbox-run: warning: no end sweeper: ...` on its stderr). A kill between the
//! sweeper's creation and its resume would leave it suspended.
//!
//! The harness runs `probe wait` (it ignores its stdin's end) as A, closes stdin, waits 2 s,
//! kills the outer, then looks at the runner processes of A's pool user right away and a few
//! times over 1.5 s (`win_suspend.rs`). The verdict is in `cancel_verdict.rs`. A suspended runner
//! it finds is ended at the row's end, so later rows' waits for that user don't stall on it.

use std::time::Duration;

use super::cancel_verdict;
use super::checks::{started, values};
use super::checks_pool::judge;
use super::launch::pool_of;
use super::rows::Ctx;
use super::sys::{now, sleep};
use super::{win, win_pool, win_suspend};

/// How long the host waits after closing the outer's stdin before it kills the outer.
const HOST_KILL: Duration = Duration::from_secs(2);
/// When the harness looks, in milliseconds after the kill.
const LOOKS: [u64; 5] = [0, 375, 750, 1_125, 1_500];
/// How long the outer's stderr may take to drain after the kill.
const DRAIN: Duration = Duration::from_secs(2);
/// The runner's image name, when the manifest's path has none.
const RUNNER: &str = "clauseo-sandbox-run.exe";
/// `ERROR_INVALID_PARAMETER` from `OpenProcess`: the process ended after the look.
const GONE: u32 = 87;

/// The `cancel` row: no runner process of A's pool user is left suspended after the host-style
/// cancel (`cancel_verdict::cancel`).
pub(crate) fn cancel(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let exe = ctx.manifest.runner_exe();
    let image = exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(RUNNER);
    let (mut a, agent) = match started(ctx, "A", &["wait"]) {
        Ok(running) => running,
        Err(why) => return judge(ctx, cancel_verdict::cancel(Err(&why), &[], &[])),
    };
    a.close_stdin();
    ctx.note(&format!(
        "harness: closed the outer's stdin as the host does on a cancel; killing the outer {} \
         {} s later",
        a.pid(),
        HOST_KILL.as_secs()
    ));
    sleep(HOST_KILL);
    a.kill();
    let killed = now();
    if let Err(why) = win_pool::enable_debug_privilege() {
        ctx.note(&format!("harness: SeDebugPrivilege not enabled: {why}"));
    }
    let mut lines = Vec::new();
    for at in LOOKS {
        sleep(Duration::from_millis(at).saturating_sub(killed.elapsed()));
        lines.extend(look(&pool.sid, image, killed.elapsed().as_millis()));
    }
    a.drain(now() + DRAIN);
    for line in &lines {
        ctx.note(&format!("harness: {line}"));
    }
    for line in &a.stderr {
        ctx.note(&format!("A stderr: {line}"));
    }
    end_suspended(ctx, &lines);
    judge(ctx, cancel_verdict::cancel(Ok(agent), &lines, &a.stderr))
}

/// One look, `ms` after the kill: `look <ms> <n>` and a `runner` line per runner process
/// (`image`) of the user `sid` or of a user the harness couldn't read; or `suspend-error <ms>
/// <ntstatus>`.
fn look(sid: &str, image: &str, ms: u128) -> Vec<String> {
    let all = match win_suspend::thread_states() {
        Ok(all) => all,
        Err(status) => return vec![format!("suspend-error {ms} {:#x}", status.cast_unsigned())],
    };
    let runners: Vec<&win_suspend::Threads> = all
        .iter()
        .filter(|p| p.image.eq_ignore_ascii_case(image))
        .collect();
    let mut lines = vec![format!("look {ms} {}", runners.len())];
    for runner in runners {
        let state = if runner.suspended {
            "suspended"
        } else {
            "running"
        };
        let pid = runner.pid;
        match win_suspend::user_of(pid) {
            Ok(user) if user.eq_ignore_ascii_case(sid) => {
                lines.push(format!("runner {ms} {pid} {state} pool"));
            }
            Ok(_) | Err(GONE) => {}
            Err(code) => lines.push(format!("runner {ms} {pid} {state} unreadable {code}")),
        }
    }
    lines
}

/// Ends each suspended runner process the looks found, and notes it.
fn end_suspended(ctx: &Ctx<'_>, lines: &[String]) {
    let mut pids: Vec<u32> = values(lines, "runner")
        .into_iter()
        .filter_map(|rest| {
            let words: Vec<&str> = rest.split_whitespace().collect();
            match words.as_slice() {
                [_, pid, "suspended", ..] => pid.parse().ok(),
                _ => None,
            }
        })
        .collect();
    pids.sort_unstable();
    pids.dedup();
    for pid in pids {
        ctx.note(&format!(
            "the harness ended the suspended runner process {pid} at the row's end \
             (TerminateProcess gave {})",
            win::terminate(pid)
        ));
    }
}
