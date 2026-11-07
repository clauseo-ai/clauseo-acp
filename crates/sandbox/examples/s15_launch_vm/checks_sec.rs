//! The security rows on what the agent can reach around it: `runners` (its two runners'
//! processes and tokens), `breakaway` (its job), `handles` (what it inherited), `desktop` (the
//! interactive desktop and the window station's DACLs) and `pipes-same-matter` (A's pipes from a
//! second agent of A's own matter). The probes they run are in `probe_sec.rs`.

use std::collections::BTreeSet;
use std::time::Duration;

use windows::Win32::System::JobObjects::JOB_OBJECT_UILIMIT_DESKTOP;

use super::checks::{DENIED, pipes_as, run, started, values};
use super::launch::{Running, Spec, deadline, launch, pool_of, run_command, summary};
use super::manifest::SECOND_A;
use super::pool_verdict;
use super::probe_sec::{PIPE_OBJECT_PREFIX, REFUSED, THREAD_RIGHTS, TOKEN_RIGHTS};
use super::rows::Ctx;
use super::sys::say;
use super::win_desk::{self, UserObject};
use super::win_thread;
use super::{win, win_pool};

/// The start of the agent's own window station's name (the outer runner makes it).
const STATION_PREFIX: &str = "ClauseoRun-";
/// `ERROR_INVALID_PARAMETER`: the pid names no process, so a refusal proves nothing.
const NO_SUCH_PROCESS: &str = "87";

/// The `runners` row: A's agent is refused every right in [`REFUSED`] and [`TOKEN_RIGHTS`] on
/// its inner runner (its parent) and on its outer runner (the process the harness started), every
/// right in [`THREAD_RIGHTS`] on each of their threads (the ids from the harness's own snapshot),
/// and the process and token rights on any `conhost.exe` either started.
pub(crate) fn runners(ctx: &Ctx<'_>) -> Result<String, String> {
    reach(ctx, &[])
}

/// The `lawyer-processes` row: the same against the lawyer's processes: the outer runner, the
/// host (the harness itself, standing in for Clauseo.exe, which starts the outer runner) and the
/// proxy (a process the harness starts as the lawyer, standing in for the egress proxy's), with
/// the runners' threads and console hosts as in `runners`.
pub(crate) fn lawyer_processes(ctx: &Ctx<'_>) -> Result<String, String> {
    let stand_in = ctx.manifest.harness_exe();
    let mut proxy = run_command(std::process::Command::new(&stand_in).args(["probe", "wait"]))?;
    let host = std::process::id();
    ctx.note(&format!(
        "host stand-in: this harness ({host}); proxy stand-in: `probe wait` as the lawyer ({})",
        proxy.pid()
    ));
    let result = reach(ctx, &[("host", host), ("proxy", proxy.pid())]);
    proxy.kill();
    result
}

/// The thread ids the harness found for a runner: its role, its pid, and the ids or the
/// snapshot's code.
type Given = (&'static str, u32, Result<Vec<u32>, u32>);

/// Starts A's `probe runners`; once it printed its parent (the inner runner), hands it the outer
/// runner, `extra` targets, and from the harness's own Toolhelp snapshots (the lawyer's, outside
/// the job) both runners' console hosts and thread ids, in one write; checks every right in
/// [`REFUSED`] and [`TOKEN_RIGHTS`] was refused on each target (console hosts included) and
/// every right in [`THREAD_RIGHTS`] on each thread.
fn reach(ctx: &Ctx<'_>, extra: &[(&str, u32)]) -> Result<String, String> {
    let (mut a, agent) = started(ctx, "A", &["runners"])?;
    let outer = a.pid();
    let until = deadline(ctx.limit);
    let parent = a.line_after("parent ", until);
    let mut targets: Vec<(String, u32)> = vec![("outer".to_owned(), outer)];
    targets.extend(extra.iter().map(|(role, pid)| ((*role).to_owned(), *pid)));
    let mut given: Vec<Given> = Vec::new();
    if let Some(inner) = parent.as_deref().and_then(|p| p.trim().parse::<u32>().ok()) {
        given.push(("inner", inner, win_thread::threads_of(inner)));
    }
    given.push(("outer", outer, win_thread::threads_of(outer)));
    let runner_pids: Vec<u32> = given.iter().map(|(_, pid, _)| *pid).collect();
    let consoles = win_thread::children_named(&runner_pids, "conhost.exe");
    let mut lines: Vec<String> = targets
        .iter()
        .map(|(role, pid)| format!("{role} {pid}"))
        .collect();
    for (_, pid) in consoles.iter().flatten() {
        lines.push(format!("conhost {pid}"));
    }
    for (role, _, tids) in &given {
        if let Ok(tids) = tids {
            let ids: Vec<String> = tids.iter().map(u32::to_string).collect();
            lines.push(format!("threads {role} {}", ids.join(" ")));
        }
    }
    lines.push("end\n".to_owned());
    a.send(&lines.join("\n"))?;
    // The probe reads up to `end`, then checks and exits; its stdin stays open until then.
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
    if extra.is_empty() {
        targets.insert(0, ("inner".to_owned(), inner));
    }
    let mut bad = Vec::new();
    if !given
        .iter()
        .any(|(role, pid, _)| *role == "inner" && *pid == inner)
    {
        bad.push(format!("inner ({inner}): the harness sent no thread ids"));
    }
    let threads = threads(ctx, &done.stdout, &given, &mut bad);
    targets.extend(conhosts(ctx, &done.stdout, &given, &consoles, &mut bad));
    let mut seen = 0usize;
    for (role, pid) in &targets {
        for (right, _) in REFUSED.iter().chain(&TOKEN_RIGHTS) {
            let key = format!("runner {role} {right}");
            let code = values(&done.stdout, &key)
                .first()
                .and_then(|v| v.split_whitespace().next())
                .map(str::to_owned);
            match code.as_deref() {
                None => bad.push(format!("{role} {right}: no line")),
                Some("0") if *right == "PROCESS_QUERY_LIMITED_INFORMATION" && role != "inner" => {
                    bad.push(format!(
                        "{role} ({pid}) {right}: GRANTED by the lawyer's default DACL (report \
                         it; not fixed here)"
                    ));
                }
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
            ctx.note(&format!("information: {line}"));
        }
    }
    if !bad.is_empty() {
        return Err(format!("the agent reached a process: {bad:?}"));
    }
    let names: Vec<String> = targets
        .iter()
        .map(|(role, pid)| format!("{role} ({pid})"))
        .collect();
    Ok(format!(
        "all {seen} opens were refused, processes and tokens, on {}; {threads}",
        names.join(", ")
    ))
}

/// Checks the agent's `thread` lines for each runner in `given` (the ids the harness's own
/// snapshot found and sent): a failed snapshot, or a live runner with no thread id, proves
/// nothing and fails; every right in [`THREAD_RIGHTS`] on every id must be refused (87, a thread
/// that ended meanwhile, is counted apart; at least one open per runner must be refused with
/// another code). The agent's own snapshot count is information. Its summary.
fn threads(ctx: &Ctx<'_>, lines: &[String], given: &[Given], bad: &mut Vec<String>) -> String {
    let mut summary = Vec::new();
    for (role, pid, tids) in given {
        let own = values(lines, &format!("snapshot {role} {pid}"))
            .first()
            .map_or_else(|| "no line".to_owned(), |v| (*v).to_owned());
        ctx.note(&format!(
            "information: the agent's own thread snapshot found {own} for the {role} runner"
        ));
        let tids = match tids {
            Ok(tids) => tids,
            Err(code) => {
                bad.push(format!(
                    "{role} ({pid}): the harness's thread snapshot failed ({code})"
                ));
                continue;
            }
        };
        ctx.note(&format!(
            "{role} runner ({pid}): {} threads in the harness's snapshot",
            tids.len()
        ));
        if tids.is_empty() {
            bad.push(format!(
                "{role} ({pid}): the harness found 0 threads for a live pid (proves nothing)"
            ));
            continue;
        }
        let (mut refused, mut ended) = (0usize, 0usize);
        for tid in tids {
            let Some(line) = values(lines, &format!("thread {role} {tid}"))
                .first()
                .copied()
            else {
                bad.push(format!("{role} ({pid}) thread {tid}: no line"));
                continue;
            };
            let words: Vec<&str> = line.split_whitespace().collect();
            for (right, _) in THREAD_RIGHTS {
                let code = words
                    .iter()
                    .position(|w| *w == right)
                    .and_then(|at| words.get(at + 1));
                match code.copied() {
                    None => bad.push(format!("{role} thread {tid} {right}: no code")),
                    Some("0") => bad.push(format!("{role} ({pid}) thread {tid} {right}: OPENED")),
                    Some(NO_SUCH_PROCESS) => ended += 1,
                    Some(_) => refused += 1,
                }
            }
        }
        if refused == 0 {
            bad.push(format!(
                "{role} ({pid}): no thread open refused with a code other than 87 (proves \
                 nothing)"
            ));
        }
        summary.push(format!(
            "{role} {} threads ({refused} thread opens refused, {ended} on threads that ended)",
            tids.len()
        ));
    }
    format!("threads (ids from the harness): {}", summary.join(", "))
}

/// The console hosts the harness's own snapshot found under the inner or the outer runner (and
/// sent), as targets (`conhost-<pid>`); none is noted as information (both runner exes are
/// GUI-subsystem, so none is expected), and so is what the agent's own snapshot found. A failed
/// harness snapshot fails the row: nothing would be known.
fn conhosts(
    ctx: &Ctx<'_>,
    lines: &[String],
    given: &[Given],
    consoles: &Result<Vec<(u32, u32)>, u32>,
    bad: &mut Vec<String>,
) -> Vec<(String, u32)> {
    ctx.note(&format!(
        "information: the agent's own process snapshot found conhost {:?}",
        values(lines, "conhost-snapshot")
    ));
    let found = match consoles {
        Ok(found) => found,
        Err(code) => {
            bad.push(format!("the harness's process snapshot failed ({code})"));
            return Vec::new();
        }
    };
    if found.is_empty() {
        ctx.note(
            "conhost none (harness snapshot; both runner exes are GUI-subsystem: none is \
             expected)",
        );
    }
    found
        .iter()
        .map(|(parent, pid)| {
            let role = given
                .iter()
                .find(|(_, p, _)| p == parent)
                .map_or("?", |(role, _, _)| *role);
            ctx.note(&format!(
                "conhost {pid} under the {role} runner (harness snapshot): tested"
            ));
            (format!("conhost-{pid}"), *pid)
        })
        .collect()
}

/// The `breakaway` row: `CREATE_BREAKAWAY_FROM_JOB` fails with 5, or the child it made is
/// still in the session's own kill-on-close job (`pool_verdict::breakaway`); the child is killed
/// either way. While the probe waits, the harness duplicates that job's handle from the outer
/// runner and checks the child with `IsProcessInJob` (or the job's pid list), then closes it.
pub(crate) fn breakaway(ctx: &Ctx<'_>) -> Result<String, String> {
    let mut a = launch(ctx, "A", &Spec::probe(ctx.manifest, &["breakaway"]))?;
    let until = deadline(ctx.limit);
    let first = a.line_after("breakaway ", until);
    let pid = first
        .as_deref()
        .and_then(|line| line.strip_prefix("child "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|p| p.parse::<u32>().ok());
    let mut ours = Vec::new();
    if let Some(pid) = pid {
        let inside = win_pool::session_job(a.pid()).and_then(|job| {
            let inside = win_pool::pid_in_job(pid, job.0);
            // Closed before the session ends, which the job's last handle must do.
            drop(job);
            inside
        });
        ours.push(format!(
            "breakaway our-job {}",
            inside.map_or_else(|why| format!("error {why}"), |inside| inside.to_string())
        ));
    }
    // The probe exits after the line, its stdin held open until then; without it, its stdin's
    // end.
    let done = match a.send("go\n") {
        Ok(()) => a.finish(until)?,
        Err(why) => {
            ctx.note(&format!(
                "the line that ends the probe's wait wasn't sent: {why}"
            ));
            a.end(until)?
        }
    };
    ctx.note(&format!("A: probe breakaway: {}", summary(&done)));
    let mut lines = done.stdout;
    lines.extend(ours);
    for line in lines.iter().chain(&done.stderr) {
        ctx.note(&format!("A: {line}"));
    }
    let killed = values(&lines, "breakaway killed").join(" ");
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
    pool_verdict::breakaway(&lines)
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
    let running = launch(ctx, "A", &Spec::probe(ctx.manifest, &["desktop"]))?;
    ctx.note(&format!("A: probe desktop, outer pid {}", running.pid()));
    Ok(running)
}

/// The `pipes-same-matter` row: a second session of A's matter gets 5, not 231, opening each of
/// A's four pipes. A pool user serves one live session at a time (the runner's first act ends
/// every other process of its user), so the second session runs as matter A's second pool user
/// (`A2`, granted A's matter folder too), as the host would run it.
pub(crate) fn pipes_same_matter(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, _pid) = started(ctx, "A", &["wait"])?;
    let result = pipes_as(ctx, SECOND_A);
    a.kill();
    result.map(|note| format!("{note} (a second session of A's matter, as its second pool user)"))
}
