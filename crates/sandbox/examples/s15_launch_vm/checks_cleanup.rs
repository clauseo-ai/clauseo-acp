//! The `end-cleanup` row: what is left of the pool user's own objects after a session. The items
//! are planted before the session, so for them it is the next session: its inner runner's first
//! act removes the HKCU `Run` and `RunOnce` values and the Startup entries, and at its end the
//! outer asks for the `SYSTEM` task `\Clauseo\SandboxSweep`, which deletes the pool user's
//! scheduled tasks and cancels its BITS jobs.
//! Before the session, the harness, logged on as A's pool user (`win_logon.rs`), plants benign
//! items with `probe items plant` (`probe_items.rs`): HKCU `Run` and `RunOnce` values, a
//! Startup-folder `.cmd` and a scheduled task (2099, never run), each running the probe's
//! `wait`, and a resumed BITS job copying a local file, whose notify command writes a marker.
//! After the session ends (the outer, living on, has polled the sweep task's result for up to
//! 20 s) it checks, as that user again, until every item is gone or [`SWEEP_GRACE`] after the
//! session's end has passed. At the row's end it removes whatever is left and notes it. Each
//! `probe items` run first waits for that user to have no process left. The verdict is in
//! `cleanup_verdict.rs`.

use std::time::Duration;

use super::checks::started;
use super::checks_pool::{judge, logon_parts, settle};
use super::checks_ui::random;
use super::cleanup_verdict::{self, KINDS, settled};
use super::launch::{deadline, pool_of, summary};
use super::manifest::Pool;
use super::rows::Ctx;
use super::sys::{now, sleep};
use super::win_logon;

/// How long after the session's end the checks are retried: the outer has polled the sweep
/// task's result for up to 20 s before it ended, and 30 s more covers a task still running then,
/// within the row's 120 s limit (the task's own limit, `PT2M`, doesn't fit in it).
const SWEEP_GRACE: Duration = Duration::from_secs(30);
/// How long one `probe items` run as the pool user may take.
const ITEMS_LIMIT: Duration = Duration::from_secs(30);

/// Runs `probe items <step> <tag>` as `pool` (`CreateProcessWithLogonW`, outside any job of
/// ours, on a control station of its own); its lines, noted. The call that failed otherwise.
/// It waits for that user to have no process left first (`checks_pool::settle`): the session's
/// processes may still be ending, and a process of that user outside the job is what a session's
/// sweep ends.
fn items(ctx: &Ctx<'_>, pool: &Pool, step: &str, tag: &str) -> Result<Vec<String>, String> {
    settle(ctx, pool)?;
    let (held, desktop, password) = logon_parts(ctx, pool)?;
    let ran = win_logon::run_with_logon(
        &pool.name,
        password,
        &desktop,
        &ctx.manifest.harness_exe(),
        &["probe", "items", step, tag],
        &ctx.manifest.bin(),
        ITEMS_LIMIT.min(ctx.limit),
    );
    drop(held);
    let ran = ran?;
    ctx.note(&format!(
        "harness: probe items {step} as {}: pid {}, exit {:?}",
        pool.name, ran.pid, ran.code
    ));
    for line in &ran.lines {
        ctx.note(&format!("{step}: {line}"));
    }
    Ok(ran.lines)
}

/// A short session of A, ended at once: the outer's stderr lines, or why it didn't run.
fn short_session(ctx: &Ctx<'_>, until: std::time::Instant) -> Result<Vec<String>, String> {
    let (a, agent) = started(ctx, "A", &["hold"])?;
    ctx.note(&format!("the session's agent {agent}; ending the session"));
    let done = a.end(until)?;
    ctx.note(&format!("A: probe hold: {}", summary(&done)));
    for line in &done.stderr {
        ctx.note(&format!("A stderr: {line}"));
    }
    Ok(done.stderr)
}

/// The `end-cleanup` row: every item planted as A's pool user before the session is gone
/// within [`SWEEP_GRACE`] of its end, or the BITS job was refused (`cleanup_verdict`).
pub(crate) fn end_cleanup(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let tag = random()?;
    let until = deadline(ctx.limit);
    let planted = items(ctx, pool, "plant", &tag)?;
    let session = short_session(ctx, until);
    let ended = now();
    let mut after = Vec::new();
    while session.is_ok() {
        // A check that fails is tried again until the grace is over: the sweep task may still
        // run after the outer stopped polling its result.
        after = items(ctx, pool, "check", &tag).unwrap_or_else(|why| {
            ctx.note(&format!("the check didn't run: {why}"));
            Vec::new()
        });
        if settled(&planted, &after) || ended.elapsed() >= SWEEP_GRACE {
            break;
        }
        sleep(Duration::from_secs(3));
    }
    let waited = ended.elapsed().as_millis();
    match items(ctx, pool, "remove", &tag) {
        Ok(lines) => {
            let removed: Vec<&str> = KINDS
                .into_iter()
                .filter(|kind| lines.contains(&format!("item {kind} removed")))
                .collect();
            if lines.iter().any(|l| l.starts_with("item bits error")) {
                ctx.note(
                    "the harness couldn't cancel the BITS job: it is left for the next sweep task",
                );
            }
            if !removed.is_empty() {
                ctx.note(&format!(
                    "the harness removed its own items left at the row's end: {}",
                    removed.join(", ")
                ));
            }
        }
        Err(why) => ctx.note(&format!("the harness's removal didn't run: {why}")),
    }
    let stderr = session?;
    let verdict = cleanup_verdict::end_cleanup(&planted, &after, &stderr, waited);
    judge(ctx, verdict)
}
