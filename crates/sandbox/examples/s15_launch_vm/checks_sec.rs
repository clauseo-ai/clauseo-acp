//! The security rows on what the agent can reach around it: `runners` (its two runners'
//! processes and tokens), `breakaway` (its job), `handles` (what it inherited), `desktop` (the
//! interactive desktop and the window station's DACLs) and `pipes-same-matter` (A's pipes from a
//! second agent of A's own matter). The probes they run are in `probe_sec.rs`.

use std::collections::BTreeSet;
use std::time::Duration;

use windows::Win32::System::JobObjects::JOB_OBJECT_UILIMIT_DESKTOP;

use super::checks::{DENIED, pipes_as, run, started, values};
use super::launch::{Running, Spec, deadline, launch, pool_of};
use super::probe_sec::{PIPE_OBJECT_PREFIX, REFUSED, TOKEN_RIGHTS};
use super::rows::Ctx;
use super::sys::say;
use super::win;
use super::win_desk::{self, UserObject};

/// The start of the agent's own window station's name (the outer runner makes it).
const STATION_PREFIX: &str = "ClauseoRun-";
/// `ERROR_INVALID_PARAMETER`: the pid names no process, so a refusal proves nothing.
const NO_SUCH_PROCESS: &str = "87";

/// The `runners` row: A's agent is refused every right in [`REFUSED`] and [`TOKEN_RIGHTS`] on
/// its inner runner (its parent) and on its outer runner (the process the harness started).
pub(crate) fn runners(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, agent) = started(ctx, "A", &["runners"])?;
    let outer = a.pid();
    let until = deadline(ctx.limit);
    let parent = a.line_after("parent ", until);
    a.send(&format!("outer {outer}\n"))?;
    let done = a.finish(until)?;
    for line in done.stdout.iter().chain(&done.stderr) {
        ctx.note(&format!("A: {line}"));
    }
    let inner = parent
        .and_then(|p| p.trim().parse::<u32>().ok())
        .ok_or_else(|| format!("the agent printed no parent pid: {:?}", done.stdout))?;
    if inner == 0 || inner == outer || inner == agent {
        return Err(format!(
            "the agent's parent is {inner} (outer {outer}, agent {agent}): not an inner runner"
        ));
    }
    ctx.note(&format!(
        "agent {agent}, its inner runner {inner}, the outer runner {outer}"
    ));
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for (role, pid) in [("inner", inner), ("outer", outer)] {
        for (right, _) in REFUSED.iter().chain(&TOKEN_RIGHTS) {
            let key = format!("runner {role} {right}");
            let code = values(&done.stdout, &key)
                .first()
                .and_then(|v| v.split_whitespace().next())
                .map(str::to_owned);
            match code.as_deref() {
                None => bad.push(format!("{role} {right}: no line")),
                Some("0") => bad.push(format!("{role} ({pid}) {right}: OPENED")),
                Some(NO_SUCH_PROCESS) => bad.push(format!(
                    "{role} ({pid}) {right}: 87, no such process (the check proves nothing)"
                )),
                Some(_) => seen += 1,
            }
        }
        for line in values(&done.stdout, "runner-info")
            .iter()
            .filter(|l| l.starts_with(role))
        {
            ctx.note(&format!("information, may be granted: {line}"));
        }
    }
    if !bad.is_empty() {
        return Err(format!("the agent reached a runner: {bad:?}"));
    }
    Ok(format!(
        "all {seen} opens of the inner ({inner}) and outer ({outer}) runners and their tokens \
         were refused"
    ))
}

/// The `breakaway` row: `CREATE_BREAKAWAY_FROM_JOB` fails with 5, or the child it made is still
/// in a job; the child is killed either way.
pub(crate) fn breakaway(ctx: &Ctx<'_>) -> Result<String, String> {
    let done = run(ctx, "A", &Spec::probe(ctx.manifest, &["breakaway"]))?;
    for line in &done.stdout {
        ctx.note(&format!("A: {line}"));
    }
    if let Some(code) = values(&done.stdout, "breakaway error").first() {
        if *code == DENIED {
            return Ok("CreateProcess with CREATE_BREAKAWAY_FROM_JOB gave 5".to_owned());
        }
        return Err(format!(
            "CreateProcess with CREATE_BREAKAWAY_FROM_JOB gave {code}, not 5"
        ));
    }
    let child = values(&done.stdout, "breakaway child")
        .first()
        .map(|v| (*v).to_owned())
        .ok_or_else(|| format!("no breakaway line: {:?} {:?}", done.stdout, done.stderr))?;
    let pid = child
        .split_whitespace()
        .next()
        .and_then(|p| p.parse::<u32>().ok());
    let killed = values(&done.stdout, "breakaway killed").join(" ");
    // The probe killed it; the harness makes sure (it may lack the right to look: then the
    // kill-on-close job, which the child is in when it passes, ends it with the outer).
    if let Some(pid) = pid {
        match win::gone_within(pid, Duration::from_secs(5)) {
            Ok(true) => ctx.note(&format!("the child {pid} is gone")),
            Ok(false) => {
                let code = win::terminate(pid);
                return Err(format!(
                    "the breakaway child {pid} still runs (TerminateProcess gave {code})"
                ));
            }
            Err(code) => ctx.note(&format!(
                "the harness can't open the child {pid} ({code}); the probe's kill gave {killed}"
            )),
        }
    }
    if child.ends_with("in-job true") {
        Ok(format!(
            "CREATE_BREAKAWAY_FROM_JOB made a child that is still in a job ({child}); killed \
             ({killed})"
        ))
    } else {
        Err(format!(
            "the CREATE_BREAKAWAY_FROM_JOB child left the job: {child}; killed ({killed})"
        ))
    }
}

/// The `handles` row: the agent holds exactly three runner pipe handles (in, out, err), never
/// `ctl`.
pub(crate) fn handles(ctx: &Ctx<'_>) -> Result<String, String> {
    let done = run(ctx, "A", &Spec::probe(ctx.manifest, &["handles"]))?;
    for line in &done.stdout {
        ctx.note(&format!("A: {line}"));
    }
    if let Some(why) = values(&done.stdout, "handles table")
        .first()
        .filter(|why| why.starts_with("error"))
    {
        return Err(format!("the agent's handle table can't be read: {why}"));
    }
    let pipes: Vec<&str> = values(&done.stdout, "handle")
        .into_iter()
        .filter_map(|v| v.split_once(" pipe ").map(|(_, rest)| rest))
        .collect();
    let field = |line: &str, key: &str| {
        let mut words = line.split_whitespace();
        words.by_ref().find(|w| *w == key);
        words.next().unwrap_or("?").to_owned()
    };
    let names: Vec<String> = pipes
        .iter()
        .map(|l| l.split_whitespace().next().unwrap_or("?").to_owned())
        .collect();
    if names.iter().all(|n| n != "?") {
        let run: Vec<&String> = names
            .iter()
            .filter(|n| n.starts_with(PIPE_OBJECT_PREFIX))
            .collect();
        let roles: BTreeSet<&str> = run.iter().filter_map(|n| n.rsplit('-').next()).collect();
        let want: BTreeSet<&str> = ["in", "out", "err"].into();
        if run.len() != 3 || roles != want {
            return Err(format!(
                "by name: the agent holds runner pipes {run:?}, not exactly in, out and err"
            ));
        }
        let others = names.len() - run.len();
        return Ok(format!(
            "by name (NtQueryObject): exactly the in, out and err runner pipes, no ctl; {others} \
             other pipe handles"
        ));
    }
    // Some names didn't come back: count the pipe handles that are a client end instead.
    let clients = pipes.iter().filter(|l| field(l, "end") == "client").count();
    if clients != 3 {
        return Err(format!(
            "by count (GetFileType and GetNamedPipeInfo, names unavailable): {clients} client \
             pipe handles, not 3: {pipes:?}"
        ));
    }
    Ok(format!(
        "by count (GetFileType and GetNamedPipeInfo; NtQueryObject gave no name): 3 client pipe \
         handles, {} pipes in all",
        pipes.len()
    ))
}

/// The `desktop` row: the agent runs on a `ClauseoRun-…` window station, can't open `WinSta0`
/// (nor, if it could, `Default` there) nor switch desktops, the job has the desktop UI limit,
/// and `Default`'s DACL has no ACE for A's pool SID. The DACLs of
/// `WinSta0` and `Default` are dumped while the agent runs; the GDI capture is information.
pub(crate) fn desktop(ctx: &Ctx<'_>) -> Result<String, String> {
    let pool = pool_of(ctx.manifest, "A")?;
    let mut a = started_desktop(ctx)?;
    let until = deadline(ctx.limit);
    let finished = a.line_after("desktop done", until).is_some();
    let station = win_desk::dacl_sddl(UserObject::WinSta0);
    let default = win_desk::dacl_sddl(UserObject::Default);
    a.kill();
    for line in a.stdout.iter().chain(&a.stderr) {
        ctx.note(&format!("A: {line}"));
    }
    if !finished {
        return Err(format!("the probe didn't finish: {:?}", a.stdout));
    }
    for (name, sddl) in [(r"WinSta0", &station), (r"WinSta0\Default", &default)] {
        match sddl {
            Ok(text) => ctx.note(&format!("{name} DACL: {text}")),
            Err(why) => ctx.note(&format!("{name} DACL can't be read: {why}")),
        }
    }
    let mut bad = Vec::new();
    match &default {
        Ok(text) => {
            for ace in win_desk::aces(text) {
                let sid = win_desk::ace_sid(&ace);
                if sid.eq_ignore_ascii_case(&pool.sid) {
                    say(&format!(
                        "ALERT desktop: the pool SID has an ACE on WinSta0\\Default: {ace}"
                    ));
                    bad.push(format!("the pool SID has an ACE on Default: {ace}"));
                } else if sid.starts_with("S-1-5-5-") {
                    ctx.note(&format!(
                        "a logon SID's ACE on Default (information): {ace}"
                    ));
                }
            }
        }
        Err(why) => bad.push(format!(
            "Default's DACL can't be read, so not checked: {why}"
        )),
    }
    let lines = &a.stdout;
    let first = |key: &str| values(lines, key).first().map(|v| (*v).to_owned());
    winsta0(ctx, &first, &mut bad);
    let limits = first("ui-limits")
        .and_then(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok());
    let switch = first("desktop switch").unwrap_or_else(|| "none".to_owned());
    match limits {
        Some(limits) if limits & JOB_OBJECT_UILIMIT_DESKTOP.0 != 0 => {
            if switch != DENIED {
                bad.push(format!(
                    "SwitchDesktop gave {switch}, not 5, under the desktop UI limit"
                ));
            }
        }
        Some(limits) => bad.push(format!(
            "no desktop UI limit (ui-limits {limits:#x}); SwitchDesktop gave {switch}"
        )),
        None => bad.push(format!(
            "the job's UI limits weren't read: {:?}",
            first("ui-limits")
        )),
    }
    let gdi = first("desktop gdi").unwrap_or_default();
    ctx.note(&format!(
        "GDI capture (information: it reaches only the agent's own desktop, {}): {gdi}",
        first("desktop own").unwrap_or_default()
    ));
    if !bad.is_empty() {
        return Err(bad.join("; "));
    }
    Ok(format!(
        "on its own station; WinSta0 not opened; SwitchDesktop gave 5 under the desktop UI \
         limit; no ACE for {} on \
         Default; GDI: {gdi}",
        pool.name
    ))
}

/// The agent's side of the screen-capture path: its own station is `ClauseoRun-…`, and
/// `WinSta0` doesn't open; if it does, `Default` on it must not open after
/// `SetProcessWindowStation`.
fn winsta0(ctx: &Ctx<'_>, first: &dyn Fn(&str) -> Option<String>, bad: &mut Vec<String>) {
    let station = first("desktop station").unwrap_or_default();
    if station.starts_with(STATION_PREFIX) {
        ctx.note(&format!("the agent's window station is {station}"));
    } else {
        bad.push(format!(
            "the agent's window station is {station:?}, not {STATION_PREFIX}…"
        ));
    }
    match first("desktop open-winsta0").as_deref() {
        Some("0") => {
            ctx.note("the agent opened WinSta0; trying Default on it");
            match (
                first("desktop set-winsta0").as_deref(),
                first("desktop open-default").as_deref(),
            ) {
                (Some("0"), Some("0")) => bad.push(
                    "the agent opened WinSta0 and, on it, WinSta0\\Default: a screen-capture path"
                        .to_owned(),
                ),
                (Some("0"), Some(code)) => {
                    ctx.note(&format!("OpenDesktopW(Default) on WinSta0 gave {code}"));
                    bad.push("the agent opened WinSta0 (Default then refused)".to_owned());
                }
                (set, open) => bad.push(format!(
                    "the agent opened WinSta0; SetProcessWindowStation {set:?}, Default {open:?}"
                )),
            }
        }
        Some(code) => ctx.note(&format!("OpenWindowStationW(WinSta0) gave {code}")),
        None => bad.push("no open-winsta0 line".to_owned()),
    }
}

/// Starts `probe desktop` as A; it waits after `desktop done`, so the DACLs are read while it
/// runs.
fn started_desktop(ctx: &Ctx<'_>) -> Result<Running, String> {
    let running = launch(ctx.manifest, "A", &Spec::probe(ctx.manifest, &["desktop"]))?;
    ctx.note(&format!("A: probe desktop, outer pid {}", running.pid()));
    Ok(running)
}

/// The `pipes-same-matter` row: a second agent of A's matter (the same pool user) gets 5, not
/// 231, opening each of A's four pipes: the pipes' Medium no-read-up label stops a Low agent of
/// the very user the DACL grants.
pub(crate) fn pipes_same_matter(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, _pid) = started(ctx, "A", &["wait"])?;
    let result = pipes_as(ctx, "A");
    a.kill();
    result.map(|note| format!("{note} (the same pool user as A: the label stopped it)"))
}
