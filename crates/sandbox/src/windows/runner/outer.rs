//! The outer runner (Windows): the desktop user's half, started by the host through
//! `spawn_in_tree` with `--launch <file>`. See `runner.rs` for the whole chain.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use super::desktop::AgentDesktop;
use super::end::{EndSignal, GRACE, SessionEnd, copy, inner_exited, relay_stdin};
use super::frame::{InnerLaunch, InnerReport, command_line, read_frame, write_frame};
use super::marker::marked;
use super::paths::{CodexReuse, WinPath};
use super::spawn::{Process, kill_on_close_job, restrict_dll_search, system_folder};
use super::start::start;
use super::stderr::{self, Detail, RunnerKind};
use super::unclean::os::check_logon;
use super::{LaunchMessage, RunExit, agent_env, codex_reuse};
use crate::windows::pipe::os::ServerPipe;
use crate::windows::pipe::{PipeSet, Role, pipe_sddl};
use crate::windows::record::current_user_sid;
use crate::windows::secrets::{SecretError, secret_for};
use crate::windows::setup::read_record_install_id;
use crate::windows::token::os::{logon_sid, own_token, profile_folder};
use crate::windows::token::{desktop_sddl, station_sddl};
use end::OsEnd;
use failure::Failure;
use launch_file::{data_dir_of, read_launch};
use steps::Start;

mod end;
mod failure;
mod launch_file;
mod steps;
mod watch;

/// The session end's guard, shared with the stdin relay's thread.
type End = Arc<SessionEnd<OsEnd>>;

/// How long the inner runner has to connect `ctl`: its logon, which makes the pool user's
/// profile the first time (about 1.7 s on the VM, `s15-spikes-userpool.md`), included.
const FIRST_CONNECT: Duration = Duration::from_secs(60);
/// How long it has for each pipe after that.
const NEXT_CONNECT: Duration = Duration::from_secs(15);
/// How long the agent's output may still drain once the inner runner exited.
const DRAIN: Duration = Duration::from_secs(2);
/// The most `Log` lines read from `ctl` while waiting for one report.
const LOG_CAP: usize = 100_000;
/// The outer runner's whole run; its exit code. `exe` is this program's own path, which the
/// inner runner runs as the pool user.
pub fn run_outer(launch_file: &Path, exe: &Path) -> u32 {
    if let Err(error) = restrict_dll_search() {
        let why = Detail::new(|| os("SetDefaultDllDirectories", &error));
        return Failure::new(RunExit::DllSearch, why).refuse();
    }
    // The runner's log file (`log_file.rs`), before anything logs.
    if let Ok(data_dir) = data_dir_of(launch_file) {
        super::log_file::install(&data_dir);
    }
    match outer(launch_file, exe) {
        Ok(code) => code,
        Err(failure) => failure.refuse(),
    }
}

fn outer(launch_file: &Path, exe: &Path) -> Result<u32, Failure> {
    // The order the launcher relies on: the launch file is read through a handle that closes
    // before `read_launch` returns (`private_dir::read_unchanged` drops it), and the outer then
    // tries to remove the file. Only later (`start.rs`) is the denial pipe connected, then the
    // inner started. The launcher deletes the launch file once the outer connects, so no handle
    // to it may stay open past here; the marker below takes only its name.
    let message = read_launch(launch_file)?;
    let data_dir = data_dir_of(launch_file)?;
    // F4, before this session's marker: a marker of this pool user's that awaits a stop refuses
    // the logon until the stop completes, one that awaits a sweep until it ran (`unclean.rs`).
    check_logon(&data_dir, &message.pool_user).map_err(|r| Failure::new(r.exit, r.why))?;
    // The unclean-session marker, before anything runs as the pool user; only a clean end in
    // `relay` removes it. One that can't be written refuses the launch with its own code
    // (`RunExit::MarkerUnwritable`): no logon follows. It records the install it belongs to; an
    // id that can't be read refuses the same way, since a marker without it could pass for an
    // older install's and be removed as stale.
    let install = read_record_install_id().map_err(|e| {
        let why = Detail::new(|| format!("the session marker failed (the install id: {e})"));
        Failure::of(RunnerKind::InstallRecordUnreadable, why)
    })?;
    marked(
        &data_dir,
        launch_file,
        &message.pool_user,
        install.as_deref(),
        |marker| marked_outer(&message, data_dir.clone(), marker, exe),
    )
    .map_err(|e| {
        let why = Detail::new(|| os("the session marker", &e));
        Failure::new(RunExit::MarkerUnwritable, why)
    })?
}

/// The run once its marker is written.
fn marked_outer(
    message: &LaunchMessage,
    data_dir: PathBuf,
    marker: PathBuf,
    exe: &Path,
) -> Result<u32, Failure> {
    let desktop = current_user_sid()
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| os("user", &e))))?;
    let exe_text = exe
        .to_str()
        .ok_or_else(|| Failure::new(RunExit::Usage, "the runner's path isn't UTF-8"))?;
    // The inner runner starts in System32, a folder the Low agent can't write (no DLL planted
    // in its current folder); it gets the agent's folder over `ctl`.
    let cwd = system_folder()
        .map_err(|e| Failure::new(RunExit::Logon, Detail::new(|| os("system folder", &e))))?;
    // The desktop user's logon SID, which the runners hold too: their process DACL grants it, so
    // one runner's sweep can end another's (`token::runner_process_sddl`).
    let logon = own_token()
        .and_then(|token| logon_sid(&token))
        .map_err(|e| Failure::new(RunExit::Token, Detail::new(|| os("logon SID", &e))))?;
    // From here every end path first ends the job, waits for it to empty, reads `SweepNext` and
    // requests the `SYSTEM` sweep task (`end.rs`).
    let end: End = Arc::new(SessionEnd::new(OsEnd {
        data_dir,
        desktop,
        logon,
        exe: exe_text.to_owned(),
        cwd,
        job: OnceLock::new(),
        marker,
        pool_user: message.pool_user.clone(),
        watch: Mutex::new(None),
    }));
    let result = session(message, &end);
    if result.is_err() {
        end.trigger(EndSignal::Failed);
        end.steps().stop_watch();
        end.finish();
    }
    match result {
        // An escapee found at the first act, a tick or the inner's end is the run's outcome, and
        // its refusal the run's one: the failure it replaces goes to runner.log by its kind.
        Err(failure) if end.escaped() && failure.exit() != RunExit::Escapee => {
            tracing::warn!(
                kind = failure.kind.name(),
                "a failure gave way to the escapee a sweep of the session found"
            );
            let replaced = failure.detail.then(|| "an escapee replaces it".to_owned());
            Err(Failure::new(RunExit::Escapee, replaced))
        }
        other => other,
    }
}

/// The launch once read: the checks, the pipes, the job, the desktop, the start of the inner
/// runner, and `serve`.
fn session(message: &LaunchMessage, end: &End) -> Result<u32, Failure> {
    let steps = end.steps();
    let (data_dir, desktop) = (&steps.data_dir, steps.desktop.clone());
    let profile = own_profile()
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| os("profile", &e))))?;
    refuse_codex_reuse(message, &profile)?;
    let env = agent_env(message, &profile)
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| e.to_string())))?;
    let env = super::ca::with_firm_ca(message, &desktop, env, stderr::notice);
    let folders = super::env::profile_folders(message)
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| e.to_string())))?;
    let set = PipeSet::random()
        .map_err(|e| Failure::new(RunExit::Pipe, Detail::new(|| os("random", &e))))?;
    let sddl = pipe_sddl(&desktop, &message.pool_user);
    let pipes = Role::ALL
        .iter()
        .map(|role| ServerPipe::create(&set.name(*role), *role, &sddl))
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| Failure::new(RunExit::Pipe, Detail::new(|| os("CreateNamedPipeW", &e))))?;
    // Held by the session end's steps, so every end path ends it and waits for it to empty
    // before the sweep task is requested (`end.rs`).
    let job = steps.hold_job(
        kill_on_close_job()
            .map_err(|e| Failure::new(RunExit::Job, Detail::new(|| os("job", &e))))?,
    );
    // Only a missing or unusable secret is the set-up's to fix; a read that failed is this
    // launch's alone, and a SID the pool doesn't hold is a bad launch.
    let password = secret_for(data_dir, &message.pool_user).map_err(|error| {
        let why = Detail::new(|| error.to_string());
        match error {
            SecretError::NoSecret { .. } => Failure::new(RunExit::NoSecret, super::PASSWORD_LOST),
            SecretError::Io(_) => Failure::new(RunExit::Logon, why),
            SecretError::NotInPool { .. } => Failure::of(RunnerKind::PoolUserMissing, why),
            SecretError::Newer { .. } | SecretError::Malformed(_) | SecretError::Dpapi { .. } => {
                Failure::new(RunExit::NoSecret, why)
            }
        }
    })?;
    // The agent's window station and desktop (`desktop.rs`), made here so the job can carry the
    // desktop UI limit: no process in it may make or switch desktops. They live as long as this
    // process.
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| {
        let why = Detail::new(|| format!("the window station's name: random failed ({error})"));
        Failure::new(RunExit::Token, why)
    })?;
    let station_name = super::station_name(random);
    let matter = message.matter_sid.as_ref();
    let agent_desktop = AgentDesktop::create(
        &station_name,
        &desktop,
        &station_sddl(&message.pool_user, matter),
        &desktop_sddl(&message.pool_user, matter),
    )
    .map_err(|(call, e)| {
        let why = Detail::new(|| os(&format!("the agent's window station: {call}"), &e));
        Failure::new(RunExit::Token, why)
    })?;
    // The denial pipe, the git watch (`outer/watch.rs`), the logon, the user's check, the job, the
    // shut, then the resume (`start.rs`); the inner's first act is the sweep of every other process
    // of the pool user's. The job's one handle stays this process's, so the job's processes end
    // with it.
    let mut steps = Start {
        pool_name: &message.pool_name,
        pool_user: &message.pool_user,
        desktop: &desktop,
        logon: &steps.logon,
        password: Some(password),
        exe: &steps.exe,
        base: set.base(),
        cwd: &steps.cwd,
        job,
        end,
        message,
        records: None,
    };
    let inner = start(&mut steps)?;
    let launch = InnerLaunch {
        pool_user: message.pool_user.clone(),
        desktop_user: desktop,
        matter_sid: message.matter_sid.clone(),
        program: message.program.clone(),
        command_line: command_line(&message.program, &message.args),
        cwd: message.cwd.clone(),
        env,
        folders,
        desktop: agent_desktop.path.clone(),
        folder_base: WinPath::parse(&message.state)
            .map(|state| state.display())
            .unwrap_or_default(),
    };
    let result = serve(&inner, pipes, &launch, end);
    // `serve` returns only on a failure (`relay` exits the process): the job ends and empties,
    // then the sweep task is requested.
    end.trigger(EndSignal::Failed);
    result
}

/// Connects and checks each pipe, sends the launch, relays the agent's stdio until the inner
/// runner exits, and gives its exit code.
fn serve(
    inner: &Process,
    pipes: Vec<ServerPipe>,
    launch: &InnerLaunch,
    end: &End,
) -> Result<u32, Failure> {
    let Ok([ctl, stdin, stdout, stderr]) = <[ServerPipe; 4]>::try_from(pipes) else {
        return Err(Failure::new(RunExit::Pipe, "four pipes expected"));
    };
    // The client is the inner runner's process, whose token user was checked before it ran. It
    // connects once its first-act sweep and cleanup are done; one that exited first said why in
    // its exit code.
    if let Err(failure) = accept(&ctl, inner, FIRST_CONNECT) {
        return Err(inner_exit(inner).unwrap_or(failure));
    }
    if let Err(error) = write_frame(&ctl, launch) {
        // An inner whose first-act sweep failed reported it and exited without reading.
        let failure = Failure::new(RunExit::Pipe, Detail::new(|| os("ctl write", &error)));
        return Err(reported(&ctl, inner, end).unwrap_or(failure));
    }
    for pipe in [&stdin, &stdout, &stderr] {
        if let Err(failure) = accept(pipe, inner, NEXT_CONNECT) {
            // An inner runner that failed before it opened its pipes said why on `ctl`.
            return Err(reported(&ctl, inner, end).unwrap_or(failure));
        }
    }
    match next_report(&ctl, end) {
        Ok(InnerReport::Started { .. }) => {}
        Ok(InnerReport::Failed { exit, detail }) => return Err(failed(inner, exit, detail)),
        Ok(InnerReport::Exited { .. } | InnerReport::Log { .. } | InnerReport::Escapee { .. }) => {
            return Err(Failure::new(
                RunExit::Pipe,
                "the inner runner skipped its start report",
            ));
        }
        Err(error) => {
            return Err(Failure::new(
                RunExit::Pipe,
                Detail::new(|| os("ctl read", &error)),
            ));
        }
    }
    relay(inner, ctl, [stdin, stdout, stderr], end)
}

/// The next report on `ctl` that isn't a `Log` notice or an `Escapee` report; each `Log`
/// notice goes to stderr, and each `Escapee` report is kept in `end` (the run's outcome) and said.
fn next_report(ctl: &ServerPipe, end: &End) -> io::Result<InnerReport> {
    for _ in 0..LOG_CAP {
        match read_frame::<InnerReport>(ctl)? {
            InnerReport::Log { kind, detail } => stderr::notice(kind, Detail::sent(detail)),
            InnerReport::Escapee {
                point,
                count,
                images,
            } => {
                end.mark_escapee();
                let detail = Detail::new(|| {
                    format!(
                        "clauseo-sandbox-run: sweep ({point}): {count} of the sandbox user's \
                         processes can't be ended ({})",
                        images.join(", ")
                    )
                });
                stderr::notice(RunnerKind::SweepEscapee, detail);
            }
            report @ (InnerReport::Started { .. }
            | InnerReport::Failed { .. }
            | InnerReport::Exited { .. }) => return Ok(report),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "too many log lines",
    ))
}

/// The failure the inner runner reported on `ctl`, if it did.
fn reported(ctl: &ServerPipe, inner: &Process, end: &End) -> Option<Failure> {
    match next_report(ctl, end) {
        Ok(InnerReport::Failed { exit, detail }) => Some(failed(inner, exit, detail)),
        Ok(
            InnerReport::Started { .. }
            | InnerReport::Exited { .. }
            | InnerReport::Log { .. }
            | InnerReport::Escapee { .. },
        )
        | Err(_) => inner_exit(inner),
    }
}

/// The inner runner's own failure from its exit code, once it exited with one of [`RunExit`]'s.
fn inner_exit(inner: &Process) -> Option<Failure> {
    let code = inner.wait(Some(Duration::ZERO)).ok().flatten()?;
    let exit = RunExit::from_code(code)?;
    let why = Detail::new(|| format!("the inner runner exited (0x{code:08X})"));
    Some(Failure::new(exit, why))
}

/// The inner runner's own failure, after it exited: its `RunExit` code's kind when it is one,
/// else [`RunnerKind::Internal`] (whose exit is `Spawn`).
fn failed(inner: &Process, exit: u32, detail: Option<String>) -> Failure {
    drop(inner.wait(Some(NEXT_CONNECT)));
    Failure::of(RunnerKind::for_code(exit), Detail::sent(detail))
}

/// Waits for `pipe`'s client and refuses one that isn't the inner runner's process.
fn accept(pipe: &ServerPipe, inner: &Process, timeout: Duration) -> Result<(), Failure> {
    let refused = |kind: RunnerKind, why: Detail| {
        inner.kill(RunExit::Pipe.code());
        Failure::of(kind, why)
    };
    pipe.connect(inner.handle.0, timeout).map_err(|e| {
        let why = Detail::new(|| os(pipe.role().suffix(), &e));
        refused(RunnerKind::Pipe, why)
    })?;
    let pid = pipe.client_pid().map_err(|e| {
        let why = Detail::new(|| os("client pid", &e));
        refused(RunnerKind::Pipe, why)
    })?;
    if pid != inner.pid {
        let why = Detail::new(|| {
            format!(
                "another process connected the {} pipe",
                pipe.role().suffix()
            )
        });
        return Err(refused(RunnerKind::PipeIntruder, why));
    }
    Ok(())
}

/// Relays this process's stdin to the agent's and the agent's stdout and stderr to this
/// process's until the inner runner exits, then exits with its code. The agent's stdin pipe
/// closes when this process's stdin ends, so the agent reads the end of its input, and it has
/// [`GRACE`] to finish before the job is ended (`end::relay_stdin`). `ctl`'s `Log` lines (the
/// inner's sweeps) go to stderr meanwhile. Once the inner exited, the output has [`DRAIN`] per
/// stream to finish; then the job is ended and emptied and the sweep task requested
/// (`end::inner_exited`), which closes what a descendant left in the job held, and the output
/// gets [`DRAIN`] again. On a cancel the exit code is the one the run would have had
/// without the job's end (`end::exit_code`). A sweep of the session that found an escapee makes
/// it [`RunExit::Escapee`] (`SessionEnd::outcome`), the code it replaces said on stderr. The
/// outer then writes what the session's marker waits for (`SessionEnd::finish`), with no wait
/// for any task's run.
fn relay(
    inner: &Process,
    ctl: ServerPipe,
    [stdin, stdout, stderr]: [ServerPipe; 3],
    end: &End,
) -> Result<u32, Failure> {
    let spawned =
        |e: &io::Error| Failure::new(RunExit::Pipe, Detail::new(|| os("relay thread", e)));
    // It may stay blocked on the host's pipe; the process ends without it.
    let host_end = Arc::clone(end);
    thread::Builder::new()
        .spawn(move || {
            // When the host's stdin ends the agent's closes, and the agent has its grace.
            relay_stdin(
                io::stdin().lock(),
                AgentStdin(stdin),
                &host_end,
                GRACE,
                drop,
            );
        })
        .map_err(|e| spawned(&e))?;
    let (done, drained) = mpsc::channel();
    for (from, out) in [(stdout, true), (stderr, false)] {
        let done = done.clone();
        thread::Builder::new()
            .spawn(move || {
                // Unlocked handles: each write takes the lock alone, so nothing else waits on a
                // copy blocked in a read.
                if out {
                    copy(&from, io::stdout());
                } else {
                    copy(&from, stderr::agent_stream());
                }
                let _sent = done.send(());
            })
            .map_err(|e| spawned(&e))?;
    }
    drop(done);
    let (report, reports) = mpsc::channel();
    let reader_end = Arc::clone(end);
    thread::Builder::new()
        .spawn(move || {
            if let Ok(last) = next_report(&ctl, &reader_end) {
                let _sent = report.send(last);
            }
        })
        .map_err(|e| spawned(&e))?;
    let ended = inner.wait(None).ok().flatten();
    // The inner has exited, so `ctl`'s reader has its last report or none soon.
    let last = reports.recv_timeout(DRAIN).ok();
    let reported = match &last {
        Some(InnerReport::Exited { code }) => Some(*code),
        _ => None,
    };
    let mut left = 2;
    let code = inner_exited(end, ended, reported, || {
        while left > 0 && drained.recv_timeout(DRAIN).is_ok() {
            left -= 1;
        }
    });
    end.steps().stop_watch();
    tell_status(last.as_ref(), code);
    end.finish();
    let outcome = end.outcome(code);
    // The run's one refusal, when it ends at one of the runner's own codes: an escapee (which
    // replaces even the agent's 0), an agent code in the runner's range, a failed wait.
    if RunExit::from_code(outcome).is_some() {
        let why = Detail::new(|| format!("the run would have exited 0x{code:08X}"));
        stderr::refuse(RunnerKind::for_code(outcome), why);
    }
    exit_now(outcome)
}

/// The agent's stdin pipe, owned, so that closing it is dropping it.
struct AgentStdin(ServerPipe);

impl Write for AgentStdin {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.0).flush()
    }
}

/// One line on stderr when the run ended at a Windows status (`0xC0000000` and up), naming
/// which process ended so: the agent (its `Exited` report came on `ctl`) or the inner runner.
/// `0xC0000142` (`STATUS_DLL_INIT_FAILED`) at start means a DLL, user32 most often, couldn't
/// start in it: a window station or desktop its token can't open.
fn tell_status(report: Option<&InnerReport>, code: u32) {
    match report {
        Some(InnerReport::Exited { code: agent }) if *agent >= 0xC000_0000 => {
            let why = Detail::new(|| format!("the agent ended at Windows status 0x{agent:08X}"));
            stderr::notice(RunnerKind::AgentWindowsStatus, why);
        }
        Some(InnerReport::Exited { .. }) => {}
        _ if code >= 0xC000_0000 => {
            let why =
                Detail::new(|| format!("the inner runner ended at Windows status 0x{code:08X}"));
            stderr::notice(RunnerKind::InnerWindowsStatus, why);
        }
        _ => {}
    }
}

/// Ends this process with `code` now: the copies still blocked on a pipe go with it, and the
/// kill-on-close job ends whatever of the agent's tree is left.
#[expect(
    clippy::exit,
    reason = "the copy threads may block on pipes; only an exit ends them"
)]
fn exit_now(code: u32) -> ! {
    // Windows takes the exit code as the u32 it is; `from_ne_bytes` keeps its bits.
    std::process::exit(i32::from_ne_bytes(code.to_ne_bytes()))
}

/// M3: the pure check, then the credential link against the lawyer's own `~\.codex\auth.json`
/// by file ID.
fn refuse_codex_reuse(message: &LaunchMessage, profile: &WinPath) -> Result<(), Failure> {
    let mut paths: Vec<(&'static str, &str)> = vec![
        ("program", &message.program),
        ("cwd", &message.cwd),
        ("state", &message.state),
    ];
    for (name, value) in [
        ("tmpdir", &message.tmpdir),
        ("ca_bundle", &message.ca_bundle),
        ("credential_link", &message.credential_link),
    ] {
        paths.extend(value.as_deref().map(|value| (name, value)));
    }
    paths.extend(message.runtime.iter().map(|p| ("runtime", p.as_str())));
    paths.extend(message.granted.iter().map(|p| ("granted", p.as_str())));
    paths.extend(message.firm_ca.iter().map(|p| ("firm_ca", p.as_str())));
    let refused =
        |reason: CodexReuse| Failure::new(RunExit::CodexReuse, Detail::new(|| reason.to_string()));
    codex_reuse(profile, paths, &message.env).map_err(refused)?;
    match &message.credential_link {
        Some(link) => {
            super::policy::credential_link_check(Path::new(link), profile).map_err(refused)
        }
        None => Ok(()),
    }
}

/// Maps a `CreateProcessWithLogonW` code to its failure.
fn logon_failure(code: u32) -> Failure {
    match code {
        1058 => Failure::new(RunExit::SecondaryLogon, super::SECLOGON_DISABLED),
        1385 => Failure::new(
            RunExit::LogonRight,
            Detail::new(|| crate::Blocked::LogonRight.to_string()),
        ),
        1326 => Failure::new(RunExit::Password, super::PASSWORD_LOST),
        other => Failure::new(
            RunExit::Logon,
            Detail::new(|| format!("CreateProcessWithLogonW failed ({other})")),
        ),
    }
}

/// This user's profile folder, from its token (never from `USERPROFILE`).
fn own_profile() -> io::Result<WinPath> {
    let text = profile_folder(&own_token()?)?;
    WinPath::parse(&text)
        .ok_or_else(|| io::Error::other("the profile folder isn't a full local path"))
}

/// `call` failed with `error`: the call and the error's kind and code, never a path.
fn os(call: &str, error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("{call} failed ({code})"),
        None => format!("{call} failed ({})", error.kind()),
    }
}
