//! The rows' bodies but codex's (`codex.rs`). Each returns a note on what it saw, or why it
//! failed; every wait ends by the row's limit.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use clauseo_sandbox::windows::ca_bundle;
use clauseo_sandbox::windows::runner::{RUN_EXIT_BASE, RunExit};
use clauseo_sandbox::windows::sids::Sid;

use super::checks_ui::ui_limits;
use super::launch::{Finished, Running, Spec, deadline, describe, launch, pool_of, summary, text};
use super::manifest::Manifest;
use super::rows::Ctx;
use super::sys::now;
use super::win;

/// `SE_GROUP_LOGON_ID`.
const LOGON_ID: u32 = 0xC000_0000;
/// `SE_GROUP_USE_FOR_DENY_ONLY`.
const DENY_ONLY: u32 = 0x10;
/// What another pool user's open must get: `ERROR_ACCESS_DENIED`.
const DENIED: &str = "5";

/// Runs `spec` as matter `which` to its end, noting its summary.
fn run(ctx: &Ctx<'_>, which: &str, spec: &Spec) -> Result<Finished, String> {
    let done = launch(ctx.manifest, which, spec)?.finish(deadline(ctx.limit))?;
    ctx.note(&format!(
        "{which}: {} {}: {}",
        spec.program,
        spec.args.join(" "),
        summary(&done)
    ));
    for line in &done.stderr {
        ctx.note(&format!("{which} stderr: {line}"));
    }
    Ok(done)
}

/// Starts the probe `args` as matter `which` and waits for its `pid` line.
fn started(ctx: &Ctx<'_>, which: &str, args: &[&str]) -> Result<(Running, u32), String> {
    let mut running = launch(ctx.manifest, which, &Spec::probe(ctx.manifest, args))?;
    let pid = running
        .line_after("pid ", deadline(ctx.limit))
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .ok_or_else(|| {
            format!(
                "{which}'s probe printed no pid; stdout {:?} stderr {:?}",
                running.stdout, running.stderr
            )
        })?;
    let first = running.first_line_ms.unwrap_or_default();
    ctx.note(&format!(
        "{which}: probe {} running, agent pid {pid}, outer pid {}, first line after {first} ms \
         (logon included)",
        args.join(" "),
        running.pid()
    ));
    Ok((running, pid))
}

/// The value after `key ` on each line of `lines` that has it.
fn values<'a>(lines: &'a [String], key: &str) -> Vec<&'a str> {
    let prefix = format!("{key} ");
    lines
        .iter()
        .filter_map(|l| l.strip_prefix(&prefix))
        .collect()
}

fn ok_exit(done: &Finished) -> Result<(), String> {
    if done.code == 0 {
        return Ok(());
    }
    Err(format!(
        "exited {}; stderr {:?}",
        describe(done.code),
        done.stderr
    ))
}

/// Row 1: whoami /all and `probe token` as A.
pub(crate) fn whoami(ctx: &Ctx<'_>) -> Result<String, String> {
    let m = ctx.manifest;
    let pool = pool_of(m, "A")?;
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_unset| r"C:\Windows".to_owned());
    let who = run(
        ctx,
        "A",
        &Spec {
            program: format!(r"{system_root}\System32\whoami.exe"),
            args: vec!["/all".to_owned()],
            ..Spec::default()
        },
    )?;
    for line in who.stdout.iter().filter(|l| !l.trim().is_empty()) {
        ctx.note(&format!("whoami: {line}"));
    }
    ok_exit(&who).map_err(|e| format!("whoami {e}"))?;
    let all = who.stdout.join("\n");
    if !all
        .to_ascii_uppercase()
        .contains(&pool.sid.to_ascii_uppercase())
    {
        return Err(format!("whoami /all doesn't name {}", pool.sid));
    }
    if !all.contains("Low Mandatory Level") {
        return Err("whoami /all doesn't say Low Mandatory Level".to_owned());
    }
    let token = run(ctx, "A", &Spec::probe(m, &["token"]))?;
    for line in &token.stdout {
        ctx.note(&format!("token: {line}"));
    }
    ok_exit(&token).map_err(|e| format!("probe token {e}"))?;
    let lines = &token.stdout;
    let user = values(lines, "user");
    if user != [pool.sid.as_str()] {
        return Err(format!("the user is {user:?}, not {}", pool.sid));
    }
    let integrity = values(lines, "integrity");
    if integrity != ["S-1-16-4096"] {
        return Err(format!(
            "the integrity is {integrity:?}, not Low (S-1-16-4096)"
        ));
    }
    let restricted: BTreeSet<&str> = values(lines, "restricted").into_iter().collect();
    let want: BTreeSet<&str> = [pool.sid.as_str(), "S-1-5-12", "S-1-5-32-545", "S-1-1-0"].into();
    if restricted != want {
        return Err(format!(
            "the restricted SIDs are {restricted:?}, not {want:?}"
        ));
    }
    let logon: Vec<(&str, u32)> = values(lines, "group")
        .into_iter()
        .filter_map(|v| {
            let (sid, attributes) = v.split_once(' ')?;
            let hex = attributes.trim_start_matches("0x");
            Some((sid, u32::from_str_radix(hex, 16).ok()?))
        })
        .filter(|(_, attributes)| attributes & LOGON_ID == LOGON_ID)
        .collect();
    if logon.is_empty() || logon.iter().any(|(_, a)| a & DENY_ONLY == 0) {
        return Err(format!("the logon SID isn't deny-only: {logon:?}"));
    }
    let privileges: Vec<&str> = values(lines, "privilege")
        .into_iter()
        .map(|v| v.split(' ').next().unwrap_or_default())
        .collect();
    if privileges != ["SeChangeNotifyPrivilege"] {
        return Err(format!(
            "the privileges are {privileges:?}, not SeChangeNotifyPrivilege alone"
        ));
    }
    let limits = ui_limits(lines)?;
    Ok(format!(
        "{} at Low; restricted {want:?}; logon SID deny-only; SeChangeNotifyPrivilege alone; \
         {limits}",
        pool.name
    ))
}

/// Row 2: node starts node with piped stdio, as A; then the harness's `probe job-ui` as node's
/// child shows node's tree runs inside the job's UI limits.
pub(crate) fn node_pipes(ctx: &Ctx<'_>) -> Result<String, String> {
    let m = ctx.manifest;
    let node = m.node.clone().ok_or("no node")?;
    // `node -e <script> <harness exe>`: the exe is process.argv[1].
    let script = "const cp=require('child_process');\
                  const c=cp.spawn(process.execPath,['-e','process.stdout.write(\"ok\")'],\
                  {stdio:'pipe'});let o='';c.stdout.on('data',d=>o+=d);\
                  c.on('error',e=>process.stdout.write('error:'+e.code+'\\n'));\
                  c.on('close',k=>{process.stdout.write('child:'+o+' code:'+k+'\\n');\
                  const j=cp.spawnSync(process.argv[1],['probe','job-ui'],{encoding:'utf8'});\
                  process.stdout.write((j.stdout||'')+(j.error?'job-ui error '+j.error.code:''));})";
    let spec = Spec {
        program: text(&node),
        args: vec!["-e".to_owned(), script.to_owned(), text(&m.harness_exe())],
        path: node.parent().map(Path::to_path_buf).into_iter().collect(),
        ..Spec::default()
    };
    let done = run(ctx, "A", &spec)?;
    for line in &done.stdout {
        ctx.note(&format!("node: {line}"));
    }
    ok_exit(&done)?;
    if !done.stdout.iter().any(|l| l.contains("child:ok")) {
        return Err(format!("node's child didn't answer ok: {:?}", done.stdout));
    }
    let limits = ui_limits(&done.stdout)?;
    Ok(format!("node's child answered ok over a pipe; {limits}"))
}

/// Row 3: B can't open A's agent.
pub(crate) fn open_process(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, pid) = started(ctx, "A", &["wait"])?;
    let done = run(
        ctx,
        "B",
        &Spec::probe(ctx.manifest, &["open-process", &pid.to_string()]),
    );
    a.kill();
    let done = done?;
    let prefix = format!("open-process {pid}");
    let code = values(&done.stdout, &prefix);
    if code != [DENIED] {
        return Err(format!(
            "B's OpenProcess on A's agent gave {code:?}, not 5: {:?}",
            done.stdout
        ));
    }
    Ok(format!("B's OpenProcess on A's agent (pid {pid}) gave 5"))
}

/// Row 4: B gets 5 on each of A's runner pipes.
pub(crate) fn pipes(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, _pid) = started(ctx, "A", &["wait"])?;
    let result = pipes_while(ctx);
    a.kill();
    result
}

fn pipes_while(ctx: &Ctx<'_>) -> Result<String, String> {
    let names = win::list_pipes("clauseo-run-").map_err(|e| format!("listing pipes: {e}"))?;
    ctx.note(&format!("pipes listed: {names:?}"));
    if names.len() < 4 {
        return Err(format!(
            "{} clauseo-run-* pipes listed, not A's four",
            names.len()
        ));
    }
    let mut args = vec!["open-pipes"];
    args.extend(names.iter().map(String::as_str));
    let done = run(ctx, "B", &Spec::probe(ctx.manifest, &args))?;
    let mut bad = Vec::new();
    for name in &names {
        let line = values(&done.stdout, &format!("pipe {name}"));
        match line.as_slice() {
            [codes] if *codes == format!("read {DENIED} write {DENIED}") => {
                ctx.note(&format!("{name}: {codes}"));
            }
            other => bad.push(format!("{name}: {other:?}")),
        }
    }
    if !bad.is_empty() {
        return Err(format!("not 5 for every open: {bad:?}"));
    }
    Ok(format!(
        "B got 5 opening each of {} pipes for read and write",
        names.len()
    ))
}

/// Row 5: killing the outer ends the agent and its child.
pub(crate) fn job(ctx: &Ctx<'_>) -> Result<String, String> {
    let (mut a, pid) = started(ctx, "A", &["spawn-child"])?;
    let child = a
        .line_after("child ", deadline(ctx.limit))
        .and_then(|c| c.trim().parse::<u32>().ok())
        .ok_or_else(|| format!("no child pid: {:?} {:?}", a.stdout, a.stderr))?;
    ctx.note(&format!(
        "agent {pid}, its child {child}; ending the outer runner {}",
        a.pid()
    ));
    let killed = now();
    a.kill();
    let until = killed + Duration::from_secs(10);
    let mut gone = Vec::new();
    for p in [pid, child] {
        let left = until.saturating_duration_since(now());
        match win::gone_within(p, left) {
            Ok(true) => gone.push(format!(
                "{p} gone after {} ms",
                killed.elapsed().as_millis()
            )),
            Ok(false) => return Err(format!("{p} still runs 10 s after the outer was killed")),
            Err(code) => return Err(format!("OpenProcess(SYNCHRONIZE) on {p} gave {code}")),
        }
    }
    Ok(format!(
        "killing the outer ended the tree: {}",
        gone.join(", ")
    ))
}

/// Row 6: stdin lines come back, and closing stdin ends the agent with 0.
pub(crate) fn stdio(ctx: &Ctx<'_>) -> Result<String, String> {
    let mut a = launch(ctx.manifest, "A", &Spec::probe(ctx.manifest, &["echo"]))?;
    let until = deadline(ctx.limit);
    for word in ["one", "two"] {
        a.send(&format!("{word}\n"))?;
        if a.line_after(&format!("echo {word}"), until).is_none() {
            return Err(format!(
                "{word:?} didn't come back: {:?} {:?}",
                a.stdout, a.stderr
            ));
        }
    }
    let first = a.first_line_ms.unwrap_or_default();
    let done = a
        .finish(until)
        .map_err(|e| format!("after stdin closed: {e} (the EOF didn't reach the agent?)"))?;
    ctx.note(&format!("{}; first line after {first} ms", summary(&done)));
    ok_exit(&done)?;
    Ok("two lines came back; stdin's EOF ended the agent with 0".to_owned())
}

/// Row 7: the agent's exit code comes back, and one in the runner's own range comes back as
/// `RunExit::AgentReserved`.
pub(crate) fn exit_code(ctx: &Ctx<'_>) -> Result<String, String> {
    let done = run(ctx, "A", &Spec::probe(ctx.manifest, &["exit", "7"]))?;
    if done.code != 7 {
        return Err(format!("the runner exited {}, not 7", describe(done.code)));
    }
    let reserved = RunExit::AgentReserved;
    let agent = RUN_EXIT_BASE + 4;
    let done = run(
        ctx,
        "A",
        &Spec::probe(ctx.manifest, &["exit", &format!("{agent:#x}")]),
    )?;
    if done.code != reserved.code() {
        return Err(format!(
            "the agent's exit {agent:#x} came back as {}, not {}",
            describe(done.code),
            describe(reserved.code())
        ));
    }
    Ok(format!(
        "exit 7 came back as 7; the agent's {agent:#x} came back as {}",
        describe(done.code)
    ))
}

/// Row 9: A reads the CA bundle and can't write it.
pub(crate) fn ca_bundle(ctx: &Ctx<'_>) -> Result<String, String> {
    let m = ctx.manifest;
    let (a, b) = m.pool_pair().ok_or("no pool pair")?;
    let parse = |sid: &str| Sid::parse(sid).map_err(|e| e.to_string());
    let desktop = parse(&win::own_token().map_err(|e| e.to_string())?.user)?;
    let folder = m.data_dir.join(ca_bundle::CA_DIR);
    let file = ca_bundle::path(&m.data_dir);
    let (had_folder, had_file) = (folder.exists(), file.exists());
    let bundle = ca_bundle::ensure_with(&m.data_dir, &desktop, &[parse(&a.sid)?, parse(&b.sid)?])
        .map_err(|e| format!("ca_bundle::ensure_with: {e}"))?;
    record_made(m, &folder, &file, had_folder, had_file)?;
    ctx.note(&format!(
        "bundle {}: {} roots, export_ms {}",
        bundle.path.display(),
        bundle.kept,
        bundle.export_ms
    ));
    let path = text(&bundle.path);
    let read = run(ctx, "A", &Spec::probe(m, &["read", &path]))?;
    if values(&read.stdout, "read")
        .first()
        .is_none_or(|v| !v.starts_with("ok "))
    {
        return Err(format!("A couldn't read the bundle: {:?}", read.stdout));
    }
    let write = run(ctx, "A", &Spec::probe(m, &["write", &path]))?;
    if values(&write.stdout, "write") != [DENIED] {
        return Err(format!(
            "A's write open of the bundle gave {:?}, not 5",
            write.stdout
        ));
    }
    Ok(format!(
        "A read the bundle ({}); its write open got 5; export_ms {}",
        values(&read.stdout, "read").join(""),
        bundle.export_ms
    ))
}

/// Records the bundle's folder (or file) in the manifest when this row made it.
fn record_made(
    m: &Manifest,
    folder: &Path,
    file: &Path,
    had_folder: bool,
    had_file: bool,
) -> Result<(), String> {
    Manifest::update(&m.root, |on_disk| {
        if !had_folder && !on_disk.dirs.iter().any(|d| d == folder) {
            on_disk.dirs.push(folder.to_path_buf());
        } else if had_folder && !had_file && !on_disk.files.iter().any(|f| f == file) {
            on_disk.files.push(file.to_path_buf());
        }
    })
}
