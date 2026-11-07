//! The `end-cleanup` row: what the runner's end-of-session sweeper (`clauseo-sandbox-run
//! --sweep`, run as the pool user after every session) leaves of the pool user's own objects.
//! Before the session, the harness, logged on as A's pool user (`win_logon.rs`), plants benign
//! items with `probe items plant` (`probe_items.rs`): HKCU `Run` and `RunOnce` values, a
//! Startup-folder `.cmd`, a scheduled task (2099, never run) and a suspended BITS job, each
//! running the probe's `wait`. After the session ends it checks, as that user again, for up to
//! [`SWEEP_GRACE`]; at the row's end it removes whatever is left and notes it. The verdict is in
//! `cleanup_verdict.rs`.

use std::time::Duration;

use clauseo_sandbox::windows::secrets::secret_for;
use clauseo_sandbox::windows::sids::Sid;

use super::checks::started;
use super::checks_pool::{end_session, judge};
use super::checks_ui::random;
use super::cleanup_verdict::{self, KINDS};
use super::launch::{deadline, pool_of};
use super::manifest::Pool;
use super::pool_verdict::Verdict;
use super::rows::Ctx;
use super::sys::{now, sleep};
use super::win_desk::ControlStation;
use super::{win, win_logon};

/// How long after the session's end the sweeper gets.
const SWEEP_GRACE: Duration = Duration::from_secs(30);
/// How long one `probe items` run as the pool user may take.
const ITEMS_LIMIT: Duration = Duration::from_secs(30);

/// Runs `probe items <step> <tag>` as `pool` (`CreateProcessWithLogonW`, outside any job of
/// ours, on a control station of its own); its lines, noted. The call that failed otherwise.
fn items(ctx: &Ctx<'_>, pool: &Pool, step: &str, tag: &str) -> Result<Vec<String>, String> {
    let lawyer = win::own_token()
        .map_err(|e| format!("the harness's token: {e}"))?
        .user;
    let station = format!("ClauseoS15Ctl-{}", random()?);
    let held = ControlStation::open(&station, &[lawyer.as_str(), pool.sid.as_str()])
        .map_err(|(call, code)| format!("{call} gave {code} (station {station})"))?;
    let sid = Sid::parse(&pool.sid).map_err(|e| e.to_string())?;
    // The crate's error text never holds the password.
    let password = secret_for(&ctx.manifest.data_dir, &sid)
        .map_err(|e| format!("secret_for({}): {e}", pool.sid))?;
    let desktop = format!("{station}\\Default");
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

/// The `end-cleanup` row: every item planted as A's pool user before the session is gone
/// within [`SWEEP_GRACE`] of its end.
pub(crate) fn end_cleanup(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let tag = random()?;
    let until = deadline(ctx.limit);
    let planted = items(ctx, pool, "plant", &tag)?;
    let session = started(ctx, "A", &["hold"]).and_then(|(a, agent)| {
        ctx.note(&format!("the session's agent {agent}; ending the session"));
        end_session(ctx, a, until)
    });
    let ended = now();
    let mut after = Vec::new();
    while session.is_ok() {
        // The sweeper may end a check of the pool user's while it runs: try again until the
        // grace is over.
        after = items(ctx, pool, "check", &tag).unwrap_or_else(|why| {
            ctx.note(&format!("the check didn't run: {why}"));
            Vec::new()
        });
        let verdict = cleanup_verdict::end_cleanup(&planted, &after, 0);
        if !matches!(verdict, Verdict::Fail(_)) || ended.elapsed() >= SWEEP_GRACE {
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
    judge(ctx, cleanup_verdict::end_cleanup(&planted, &after, waited))
}
