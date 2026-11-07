//! The `end-cleanup` row: what the runner's end-of-session sweeper (`clauseo-sandbox-run
//! --sweep`, run as the pool user after every session) leaves of the pool user's own objects.
//! Before the session, the harness, logged on as A's pool user (`win_logon.rs`), plants benign
//! items with `probe items plant` (`probe_items.rs`): HKCU `Run` and `RunOnce` values, a
//! Startup-folder `.cmd` and a scheduled task (2099, never run), each running the probe's
//! `wait`, and a resumed BITS job copying a local file, whose notify command writes a marker.
//! After the session ends it waits until that user has no process left (the sweeper has ended,
//! up to `checks_pool::SETTLE`), then checks, as that user again, until [`SWEEP_GRACE`] after
//! the session's end has passed. When the BITS job is still there (the sweeper can't cancel it:
//! BITS holds no logon token for a pool user), the row runs a second short session of the same
//! pool user and checks again: the job must still be inert, its marker never written and its
//! state never transferred. At the row's end it removes whatever is left and notes it. Each
//! `probe items` run waits that way first. The verdict is in `cleanup_verdict.rs`; once the
//! `SYSTEM` stop task lands (PR #668) the row will also require the job gone after that stop.

use std::time::Duration;

use super::checks::started;
use super::checks_pool::{end_session, judge, logon_parts, settle};
use super::checks_ui::random;
use super::cleanup_verdict::{self, KINDS, bits_left, settled};
use super::launch::{deadline, pool_of};
use super::manifest::Pool;
use super::rows::Ctx;
use super::sys::{now, sleep};
use super::win_logon;

/// How long after the session's end the sweeper gets.
const SWEEP_GRACE: Duration = Duration::from_secs(30);
/// How long one `probe items` run as the pool user may take.
const ITEMS_LIMIT: Duration = Duration::from_secs(30);

/// Runs `probe items <step> <tag>` as `pool` (`CreateProcessWithLogonW`, outside any job of
/// ours, on a control station of its own); its lines, noted. The call that failed otherwise.
/// It waits for that user to have no process left first (`checks_pool::settle`): after the
/// session, that is its end sweeper's end, which would otherwise end the check.
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

/// A short session of A, ended at once; whether it ran.
fn short_session(ctx: &Ctx<'_>, until: std::time::Instant) -> Result<(), String> {
    started(ctx, "A", &["hold"]).and_then(|(a, agent)| {
        ctx.note(&format!("the session's agent {agent}; ending the session"));
        end_session(ctx, a, until)
    })
}

/// The `end-cleanup` row: every item planted as A's pool user before the session is gone
/// within [`SWEEP_GRACE`] of its end, but for the BITS job, which may be left inert.
pub(crate) fn end_cleanup(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let tag = random()?;
    let until = deadline(ctx.limit);
    let planted = items(ctx, pool, "plant", &tag)?;
    let session = short_session(ctx, until);
    let ended = now();
    let mut after = Vec::new();
    while session.is_ok() {
        // Each check waits for the sweeper's end first; a check that fails anyway is tried again
        // until the grace is over.
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
    // A BITS job still there must stay inert through the next session of the same pool user.
    let mut again = None;
    if session.is_ok() && bits_left(&after) {
        ctx.note("the BITS job is still there: a second session, then a check again");
        match short_session(ctx, until).and_then(|()| items(ctx, pool, "check", &tag)) {
            Ok(lines) => again = Some(lines),
            Err(why) => ctx.note(&format!(
                "the second session or its check didn't run: {why}"
            )),
        }
    }
    match items(ctx, pool, "remove", &tag) {
        Ok(lines) => {
            let removed: Vec<&str> = KINDS
                .into_iter()
                .filter(|kind| lines.contains(&format!("item {kind} removed")))
                .collect();
            if lines.iter().any(|l| l.starts_with("item bits error")) {
                ctx.note(
                    "the harness couldn't cancel the BITS job either: it is left for the stop",
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
    session?;
    let verdict = cleanup_verdict::end_cleanup(&planted, &after, again.as_deref(), waited);
    judge(ctx, verdict)
}
