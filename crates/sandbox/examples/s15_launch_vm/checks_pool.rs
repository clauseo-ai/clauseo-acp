//! The rows that watch, from outside, that no process of the session's pool user runs outside
//! its kill-on-close job: `pool-processes-at-start`, `pool-processes-at-end`,
//! `sweep-before-logon` and `sweep-mid-session` (the runner's process sweep), and
//! `agent-token-config`. The harness lists the pool user's processes itself (`win_pool.rs`) and
//! starts its own benign stand-in, `probe wait` as the pool user outside any job of ours, the
//! way the positive control's relay starts (`win_logon.rs`). The verdicts are in
//! `pool_verdict.rs`.
//!
//! Every session here runs `probe hold`, which prints its pid and exits at the end of its stdin,
//! so the session ends the way a host's does and the runner's own end runs.

use std::time::{Duration, Instant};

use clauseo_sandbox::windows::secrets::secret_for;
use clauseo_sandbox::windows::sids::Sid;

use super::checks::{run, started};
use super::checks_ui::random;
use super::launch::{Running, Spec, deadline, pool_of, summary};
use super::manifest::Pool;
use super::pool_verdict::{self, MID_SESSION_MS, Verdict, logon_sids};
use super::rows::Ctx;
use super::sys::{now, sleep};
use super::win_desk::ControlStation;
use super::win_logon::{Started, start_with_logon};
use super::{win, win_pool};

/// How long after the session's end `pool-processes-at-end` waits for the listing to empty.
const END_GRACE: Duration = Duration::from_secs(10);
/// How long the benign process may take to log on and print its pid.
const BENIGN_START: Duration = Duration::from_secs(30);
/// The services `agent-token-config` records: Secondary Logon, WMI, Task Scheduler and BITS.
const SERVICES: [&str; 4] = ["seclogon", "Winmgmt", "Schedule", "BITS"];

/// The row's result from `verdict`: an INFO verdict marks the row INFO ([`Ctx::info`]).
fn judge(ctx: &Ctx<'_>, verdict: Verdict) -> Result<String, String> {
    match verdict {
        Verdict::Pass(note) => Ok(note),
        Verdict::Fail(why) => Err(why),
        Verdict::Info(why) => {
            ctx.info(&why);
            Ok("nothing could be checked".to_owned())
        }
    }
}

/// The harness's listing of `sid`'s processes as `pool_verdict.rs` reads it, each checked
/// against `job` when there is one; noted under the row. `SeDebugPrivilege` is enabled first,
/// so an admin harness can open every process and its token.
fn listing(ctx: &Ctx<'_>, sid: &str, job: Option<&win::Owned>) -> Vec<String> {
    if let Err(why) = win_pool::enable_debug_privilege() {
        ctx.note(&format!("harness: SeDebugPrivilege not enabled: {why}"));
    }
    let lines = lines_of(sid, job);
    for line in &lines {
        ctx.note(&format!("harness: {line}"));
    }
    lines
}

/// The listing's lines: `pool` per process of `sid`, `pool-unreadable <pid> <image>` per
/// process whose user couldn't be read, then `pool-unknown <n>`; or `pool-error`.
fn lines_of(sid: &str, job: Option<&win::Owned>) -> Vec<String> {
    match win_pool::pool_processes(sid) {
        Err(code) => vec![format!("pool-error the process snapshot gave {code}")],
        Ok(listing) => {
            let mut lines: Vec<String> = listing
                .found
                .iter()
                .map(|p| {
                    let in_job = match job.map(|job| win_pool::in_job(p.handle.0, job.0)) {
                        None => "unchecked".to_owned(),
                        Some(Ok(inside)) => inside.to_string(),
                        Some(Err(code)) => format!("error:{code}"),
                    };
                    format!("pool {} in-job={in_job} {}", p.pid, p.image)
                })
                .collect();
            for (pid, image) in &listing.unknown {
                lines.push(format!("pool-unreadable {pid} {image}"));
            }
            lines.push(format!("pool-unknown {}", listing.unknown.len()));
            lines
        }
    }
}

/// The listing while the session of the outer runner `outer` runs, against its kill-on-close
/// job. The job's handle is closed before this returns, so the session still ends with its outer.
fn listing_in_session(ctx: &Ctx<'_>, sid: &str, outer: u32) -> Vec<String> {
    match win_pool::session_job(outer) {
        Ok(job) => {
            let lines = listing(ctx, sid, Some(&job));
            drop(job);
            lines
        }
        Err(why) => {
            let line = format!("pool-error the session's job: {why}");
            ctx.note(&format!("harness: {line}"));
            vec![line]
        }
    }
}

/// Ends the session `a` (its stdin closed) and notes how it ended.
fn end_session(ctx: &Ctx<'_>, a: Running, until: Instant) -> Result<(), String> {
    let done = a.finish(until)?;
    ctx.note(&format!("A: probe hold: {}", summary(&done)));
    for line in &done.stderr {
        ctx.note(&format!("A stderr: {line}"));
    }
    Ok(())
}

/// The `pool-processes-at-start` row: once the agent printed its first line, every process whose
/// token user is A's pool user is in the session's kill-on-close job.
pub(crate) fn at_start(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let until = deadline(ctx.limit);
    let (a, agent) = started(ctx, "A", &["hold"])?;
    let lines = listing_in_session(ctx, &pool.sid, a.pid());
    end_session(ctx, a, until)?;
    judge(ctx, pool_verdict::at_start(&lines, agent))
}

/// The `pool-processes-at-end` row: once the session ended (up to [`END_GRACE`] later), no
/// process of A's pool user is left.
pub(crate) fn at_end(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let until = deadline(ctx.limit);
    let (a, agent) = started(ctx, "A", &["hold"])?;
    let running = listing_in_session(ctx, &pool.sid, a.pid());
    end_session(ctx, a, until)?;
    let ended = now();
    let mut after = listing(ctx, &pool.sid, None);
    while after.iter().any(|l| l.starts_with("pool ")) && ended.elapsed() < END_GRACE {
        sleep(Duration::from_millis(500));
        after = lines_of(&pool.sid, None);
    }
    let waited = ended.elapsed().as_millis();
    ctx.note(&format!("after the session ended ({waited} ms): {after:?}"));
    judge(ctx, pool_verdict::at_end(&running, agent, &after, waited))
}

/// The harness's benign stand-in for a stray process of the pool user: `probe wait` started as
/// that user (`CreateProcessWithLogonW`, outside any job of ours) on a control station of its own
/// (never `WinSta0`), which stays open while it runs.
struct Benign {
    started: Started,
    _station: ControlStation,
}

/// Starts the benign process as `pool` and waits for its pid line; the `benign ran <pid>` or
/// `benign never-ran <why>` line, and the process when it was started.
fn benign(ctx: &Ctx<'_>, pool: &Pool) -> Result<(String, Option<Benign>), String> {
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
    let exe = ctx.manifest.harness_exe();
    let args = ["probe", "wait"];
    let started = match start_with_logon(
        &pool.name,
        password,
        &desktop,
        &exe,
        &args,
        &ctx.manifest.bin(),
    ) {
        Ok(started) => started,
        Err(why) => return Ok((format!("benign never-ran {why}"), None)),
    };
    let pid = started.pid();
    let until = now() + BENIGN_START.min(ctx.limit);
    let line = loop {
        match started.line(until) {
            Some(text) if text.starts_with("pid ") => break format!("benign ran {pid}"),
            Some(_) => {}
            None => match started.exit_within(Duration::ZERO) {
                Some(code) => {
                    break format!("benign never-ran it exited {code:#x} before its pid line");
                }
                None => {
                    break format!(
                        "benign never-ran no pid line within {} s",
                        BENIGN_START.as_secs()
                    );
                }
            },
        }
    };
    ctx.note(&format!(
        "harness: probe wait as {} outside any job of ours, pid {pid}, on {desktop}: {line}",
        pool.name
    ));
    Ok((
        line,
        Some(Benign {
            started,
            _station: held,
        }),
    ))
}

/// Ends the benign process if it still runs, and notes it.
fn end_benign(ctx: &Ctx<'_>, benign: Option<Benign>) {
    if let Some(benign) = benign
        && let Some(code) = benign.started.end()
    {
        ctx.note(&format!(
            "the harness ended its own benign process {} at the row's end (TerminateProcess gave \
             {code})",
            benign.started.pid()
        ));
    }
}

/// The `benign gone <ms> exit <code>` or `benign alive <ms>` line: whether `benign`, started at
/// `since`, ends within `wait`.
fn gone_line(benign: &Benign, since: Instant, wait: Duration) -> String {
    let exit = benign.started.exit_within(wait);
    let ms = since.elapsed().as_millis();
    match exit {
        Some(code) => format!("benign gone {ms} exit {code:#x}"),
        None => format!("benign alive {ms}"),
    }
}

/// The `sweep-before-logon` row: a benign process of A's pool user (named in the launch message
/// the harness writes, so known before the session), started outside the job before the session,
/// is gone by the time the agent prints its first line.
pub(crate) fn before_logon(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let until = deadline(ctx.limit);
    let since = now();
    let (ran, held) = benign(ctx, pool)?;
    let ok = ran.starts_with("benign ran");
    let mut lines = vec![ran];
    let session = match held.as_ref().filter(|_| ok) {
        None => Ok(()),
        Some(benign) => started(ctx, "A", &["hold"]).and_then(|(a, agent)| {
            let gone = gone_line(benign, since, Duration::ZERO);
            ctx.note(&format!(
                "harness: at the agent's ({agent}) first line: {gone}"
            ));
            lines.push(gone);
            end_session(ctx, a, until)
        }),
    };
    end_benign(ctx, held);
    session?;
    judge(ctx, pool_verdict::swept(&lines, None))
}

/// The `sweep-mid-session` row: a benign process of the session's pool user, started outside the
/// job while the session runs, is gone within [`MID_SESSION_MS`].
pub(crate) fn mid_session(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let until = deadline(ctx.limit);
    let (a, agent) = started(ctx, "A", &["hold"])?;
    let since = now();
    let (ran, held) = benign(ctx, pool)?;
    let ok = ran.starts_with("benign ran");
    let mut lines = vec![ran];
    if let Some(benign) = held.as_ref().filter(|_| ok) {
        let limit = Duration::from_millis(u64::try_from(MID_SESSION_MS).unwrap_or(u64::MAX));
        let gone = gone_line(benign, since, limit.saturating_sub(since.elapsed()));
        ctx.note(&format!("harness: while the agent ({agent}) runs: {gone}"));
        lines.push(gone);
    }
    let session = end_session(ctx, a, until);
    end_benign(ctx, held);
    session?;
    judge(ctx, pool_verdict::swept(&lines, Some(MID_SESSION_MS)))
}

/// A service state's name.
fn state_name(state: u32) -> &'static str {
    match state {
        1 => "stopped",
        2 => "start-pending",
        3 => "stop-pending",
        4 => "running",
        5 => "continue-pending",
        6 => "pause-pending",
        7 => "paused",
        _ => "?",
    }
}

/// The `agent-token-config` row: the states of Secondary Logon, WMI, Task Scheduler and BITS
/// (information only), and A's agent's token from `probe token`: its restricting SIDs, Low
/// integrity and deny-only logon SID are asserted (`pool_verdict::token_config`).
pub(crate) fn token_config(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    for name in SERVICES {
        match win_pool::service_state(name) {
            Ok(state) => ctx.note(&format!(
                "information: service {name} {} ({state})",
                state_name(state)
            )),
            Err(why) => ctx.note(&format!("information: service {name} not read: {why}")),
        }
    }
    let token = run(ctx, "A", &Spec::probe(ctx.manifest, &["token"]))?;
    for line in &token.stdout {
        ctx.note(&format!("token: {line}"));
    }
    let lines = &token.stdout;
    ctx.note(&format!(
        "information: the agent's logon SID attributes {:?}",
        logon_sids(lines)
    ));
    judge(ctx, pool_verdict::token_config(lines, &pool.sid))
}
