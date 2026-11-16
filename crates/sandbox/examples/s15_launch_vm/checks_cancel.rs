//! The `cancel` row: what the host does on a cancel, and whether it leaves a runner behind.
//! The host closes the outer runner's stdin, then kills the outer 2 s later. The outer gives the
//! agent 1 s, ends the job, waits up to 400 ms for it to empty, then asks the Task Scheduler for
//! the `SYSTEM` task `\Clauseo\SandboxSweep`, which a kill of the outer doesn't stop. It starts
//! no process of the pool user at the end, so no runner process of that user may be left.
//!
//! The harness runs `probe wait` (it ignores its stdin's end) as A, closes stdin, waits 2 s,
//! kills the outer, then looks at the runner processes of A's pool user right away and a few
//! times over 1.5 s (`win_suspend.rs`). The verdict is in `cancel_verdict.rs`. A suspended runner
//! it finds is ended at the row's end, so later rows' waits for that user don't stall on it.
//!
//! The `stdin-unread-cancel` row: the same cancel with the agent's stdin write blocked. The agent
//! (`probe sleep-noread`) never reads its stdin; the harness writes 4 MiB to the outer's stdin
//! from a thread, far past a pipe's buffer, and closes it, as the host does on a cancel. The
//! outer buffers what the agent's full pipe won't take (up to 16 MiB), so the harness's write
//! ends, but the outer's own write to the agent stays blocked. Then the harness lets the ladder
//! run: 3 s after the close (or at the outer's earlier exit) it looks once, and no runner process
//! of the pool user may be left, the job's end having released the blocked write. The outer,
//! living on, polls the sweep task's result for up to 20 s, so it must exit by itself within
//! 25 s of the close; one still running then is killed. The harness looks over 3 s after the
//! outer ended.

use std::io::Write as _;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::cancel_verdict::{self, EXIT_MS, LADDER_MS};
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
/// What `stdin-unread-cancel` writes to the outer's stdin: far past a pipe's buffer.
const UNREAD: usize = 4 * 1024 * 1024;
/// How long that write may take; the outer buffers up to 16 MiB the agent hasn't read.
const WRITE_LIMIT: Duration = Duration::from_secs(20);
/// When `stdin-unread-cancel` looks, in milliseconds after the outer ended.
const AFTER: [u64; 5] = [0, 750, 1_500, 2_250, 3_000];

/// The runner's image name, from the manifest's path.
fn runner_image(ctx: &Ctx<'_>) -> String {
    let exe = ctx.manifest.runner_exe();
    exe.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(RUNNER)
        .to_owned()
}

/// The `cancel` row: no runner process of A's pool user is left after the host-style cancel
/// (`cancel_verdict::cancel`).
pub(crate) fn cancel(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let image = runner_image(ctx);
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
        lines.extend(look(&pool.sid, &image, killed.elapsed().as_millis()));
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

/// The `stdin-unread-cancel` row: no runner process of A's pool user is left [`LADDER_MS`] after
/// the stdin's close while the agent's stdin write is blocked, the outer exits by itself within
/// [`EXIT_MS`], and nothing is left after it (`cancel_verdict::stdin_unread`).
pub(crate) fn stdin_unread_cancel(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let image = runner_image(ctx);
    let (mut a, agent) = match started(ctx, "A", &["sleep-noread"]) {
        Ok(running) => running,
        Err(why) => {
            let verdict = cancel_verdict::stdin_unread(Err(&why), None, &[], &[], &[]);
            return judge(ctx, verdict);
        }
    };
    let mut stdin = a
        .take_stdin()
        .ok_or("the outer's stdin was already closed")?;
    let (sent, written) = mpsc::channel::<(Result<(), String>, Instant)>();
    std::thread::spawn(move || {
        let data = vec![b'x'; UNREAD];
        let wrote = stdin
            .write_all(&data)
            .and_then(|()| stdin.flush())
            .map_err(|e| e.to_string());
        drop(stdin);
        let _sent = sent.send((wrote, now()));
    });
    let closed = match written.recv_timeout(WRITE_LIMIT) {
        Ok((Ok(()), closed)) => closed,
        Ok((Err(why), _)) => {
            a.kill();
            return Err(format!(
                "the harness's 4 MiB write to the outer's stdin failed ({why}); stderr {:?}",
                a.stderr
            ));
        }
        Err(_) => {
            // The kill breaks the pipe, so the writer's thread ends too.
            a.kill();
            return Err(format!(
                "the harness's 4 MiB write to the outer's stdin didn't end within {} s: the \
                 outer stopped reading it; stderr {:?}",
                WRITE_LIMIT.as_secs(),
                a.stderr
            ));
        }
    };
    ctx.note(&format!(
        "harness: wrote 4 MiB the agent never reads to the outer's stdin and closed it, as the \
         host does on a cancel; looking {LADDER_MS} ms later, and waiting up to {EXIT_MS} ms \
         for the outer {} to exit",
        a.pid()
    ));
    if let Err(why) = win_pool::enable_debug_privilege() {
        ctx.note(&format!("harness: SeDebugPrivilege not enabled: {why}"));
    }
    let early = a
        .exited_by(closed + millis(LADDER_MS))
        .then(|| closed.elapsed().as_millis());
    let ladder = look(&pool.sid, &image, closed.elapsed().as_millis());
    let exited = early.or_else(|| {
        a.exited_by(closed + millis(EXIT_MS))
            .then(|| closed.elapsed().as_millis())
    });
    if exited.is_none() {
        a.kill();
    }
    let ended = now();
    let mut lines = Vec::new();
    for at in AFTER {
        sleep(Duration::from_millis(at).saturating_sub(ended.elapsed()));
        lines.extend(look(&pool.sid, &image, ended.elapsed().as_millis()));
    }
    for line in &ladder {
        ctx.note(&format!("harness: at the ladder's end: {line}"));
    }
    a.drain(now() + DRAIN);
    for line in &lines {
        ctx.note(&format!("harness: {line}"));
    }
    for line in &a.stderr {
        ctx.note(&format!("A stderr: {line}"));
    }
    end_suspended(ctx, &[ladder.as_slice(), lines.as_slice()].concat());
    let verdict = cancel_verdict::stdin_unread(Ok(agent), exited, &ladder, &lines, &a.stderr);
    judge(ctx, verdict)
}

/// `ms` milliseconds as a [`Duration`].
fn millis(ms: u128) -> Duration {
    Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX))
}

/// One look, `ms` after the kill (or the outer's exit): `look <ms> <n>` and a `runner` line per
/// runner process (`image`) of the user `sid` or of a user the harness couldn't read; or
/// `suspend-error <ms> <ntstatus>`.
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
