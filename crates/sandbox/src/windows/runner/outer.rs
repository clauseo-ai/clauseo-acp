//! The outer runner (Windows): the desktop user's half, started by the host through
//! `spawn_in_tree` with `--launch <file>`. See `runner.rs` for the whole chain.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use super::desktop::AgentDesktop;
use super::end::{EndSignal, EndSweep, GRACE, copy, inner_exited, relay_stdin};
use super::frame::{InnerLaunch, InnerReport, command_line, read_frame, write_frame};
use super::marker::{marked, remove_marker};
use super::paths::{CodexReuse, WinPath};
use super::spawn::{Process, kill_on_close_job, restrict_dll_search, system_folder};
use super::start::start;
use super::{LAUNCH_CAP, LaunchMessage, RUN_DIR, RunExit, agent_env, codex_reuse, is_launch_name};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::pipe::os::ServerPipe;
use crate::windows::pipe::{PipeSet, Role, pipe_sddl};
use crate::windows::record::current_user_sid;
use crate::windows::secrets::{SecretError, secret_for};
use crate::windows::token::os::{logon_sid, own_token, profile_folder};
use crate::windows::token::{desktop_sddl, station_sddl};
use crate::windows::{private_dir, write};
use end::OsSweeper;
use steps::Start;

mod end;
mod steps;

/// The end sweeper's guard, shared with the stdin relay's thread.
type End = Arc<EndSweep<OsSweeper>>;

/// How long the inner runner has to connect `ctl`: its logon, which makes the pool user's
/// profile the first time (about 1.7 s on the VM, `s15-spikes-userpool.md`), included.
const FIRST_CONNECT: Duration = Duration::from_secs(60);
/// How long it has for each pipe after that.
const NEXT_CONNECT: Duration = Duration::from_secs(15);
/// How long the agent's output may still drain once the inner runner exited.
const DRAIN: Duration = Duration::from_secs(2);
/// The most `Log` lines read from `ctl` while waiting for one report.
const LOG_CAP: usize = 100_000;
/// What the outer says when a sweep of its session found an escapee.
const ESCAPED: &str = "a process of the sandbox user's outlived every sweep of this session";

/// A runner failure: its exit code and what to say on stderr.
struct Failure(RunExit, String);

impl Failure {
    fn new(exit: RunExit, what: impl Into<String>) -> Failure {
        Failure(exit, what.into())
    }
}

/// The outer runner's whole run; its exit code. `exe` is this program's own path, which the
/// inner runner runs as the pool user.
pub fn run_outer(launch_file: &Path, exe: &Path) -> u32 {
    if let Err(error) = restrict_dll_search() {
        say(&format!(
            "clauseo-sandbox-run: {}",
            os("SetDefaultDllDirectories", &error)
        ));
        return RunExit::DllSearch.code();
    }
    match outer(launch_file, exe) {
        Ok(code) => code,
        Err(Failure(exit, what)) => {
            say(&format!("clauseo-sandbox-run: {what}"));
            exit.code()
        }
    }
}

fn outer(launch_file: &Path, exe: &Path) -> Result<u32, Failure> {
    let message = read_launch(launch_file)?;
    let data_dir = data_dir_of(launch_file)?;
    // The unclean-session marker, before anything runs as the pool user; only a clean end in
    // `relay` removes it. One that can't be written refuses the launch with its own code
    // (`RunExit::MarkerUnwritable`): no logon follows.
    marked(&data_dir, launch_file, &message.pool_user, |marker| {
        marked_outer(&message, data_dir.clone(), marker, exe)
    })
    .map_err(|e| Failure::new(RunExit::MarkerUnwritable, os("the session marker", &e)))?
}

/// The run once its marker is written.
fn marked_outer(
    message: &LaunchMessage,
    data_dir: PathBuf,
    marker: PathBuf,
    exe: &Path,
) -> Result<u32, Failure> {
    let desktop =
        current_user_sid().map_err(|e| Failure::new(RunExit::BadLaunch, os("user", &e)))?;
    let exe_text = exe
        .to_str()
        .ok_or_else(|| Failure::new(RunExit::Usage, "the runner's path isn't UTF-8"))?;
    // The inner runner and the end sweeper start in System32, a folder the Low agent can't
    // write (no DLL planted in their current folder); the inner gets the agent's folder over
    // `ctl`.
    let cwd = system_folder().map_err(|e| Failure::new(RunExit::Logon, os("system folder", &e)))?;
    // The desktop user's logon SID, which the runners hold too: their process DACL grants it, so
    // one runner's sweep can end another's (`token::runner_process_sddl`).
    let logon = own_token()
        .and_then(|token| logon_sid(&token))
        .map_err(|e| Failure::new(RunExit::Token, os("logon SID", &e)))?;
    // From here every end path first ends the job, waits for it to empty, and starts the
    // detached end sweeper (`end.rs`).
    let end: End = Arc::new(EndSweep::new(OsSweeper {
        data_dir,
        pool_user: message.pool_user.clone(),
        pool_name: message.pool_name.clone(),
        desktop,
        logon,
        exe: exe_text.to_owned(),
        cwd,
        kept: Mutex::new(None),
        job: OnceLock::new(),
        marker,
    }));
    let result = session(message, &end);
    if result.is_err() {
        end.trigger(EndSignal::Failed);
        end.wait();
    }
    match result {
        // An escapee found at a tick, at the end or by the end sweeper is the run's outcome;
        // the failure is said first.
        Err(Failure(exit, what)) if end.escaped() && exit != RunExit::Escapee => {
            say(&format!("clauseo-sandbox-run: {what}"));
            Err(Failure::new(RunExit::Escapee, ESCAPED))
        }
        other => other,
    }
}

/// The launch once read: the checks, the pipes, the job, the desktop, the start of the inner
/// runner, and `serve`.
fn session(message: &LaunchMessage, end: &End) -> Result<u32, Failure> {
    let sweeper = end.steps();
    let (data_dir, desktop) = (&sweeper.data_dir, sweeper.desktop.clone());
    let profile = own_profile().map_err(|e| Failure::new(RunExit::BadLaunch, os("profile", &e)))?;
    refuse_codex_reuse(message, &profile)?;
    // Held until this process exits: msys's folders for the tree go with their last handle.
    let git_bash = super::git_bash::os::for_run(message, &desktop, say);
    let env = agent_env(message, &profile, &git_bash.env)
        .map_err(|e| Failure::new(RunExit::BadLaunch, e.to_string()))?;
    let env = super::ca::with_firm_ca(message, &desktop, env, say);
    let folders = super::env::profile_folders(message)
        .map_err(|e| Failure::new(RunExit::BadLaunch, e.to_string()))?;
    let set = PipeSet::random().map_err(|e| Failure::new(RunExit::Pipe, os("random", &e)))?;
    let sddl = pipe_sddl(&desktop, &message.pool_user);
    let pipes = Role::ALL
        .iter()
        .map(|role| ServerPipe::create(&set.name(*role), *role, &sddl))
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| Failure::new(RunExit::Pipe, os("CreateNamedPipeW", &e)))?;
    // Held by the end sweeper's steps, so every end path ends it and waits for it to empty
    // before the sweeper starts (`end.rs`).
    let job = sweeper
        .hold_job(kill_on_close_job().map_err(|e| Failure::new(RunExit::Job, os("job", &e)))?);
    // Only a missing or unusable secret is the set-up's to fix; a read that failed is this
    // launch's alone, and a SID the pool doesn't hold is a bad launch.
    let password = secret_for(data_dir, &message.pool_user).map_err(|error| match error {
        SecretError::NoSecret { .. } => Failure::new(RunExit::NoSecret, super::PASSWORD_LOST),
        SecretError::Io(_) => Failure::new(RunExit::Logon, error.to_string()),
        SecretError::NotInPool { .. } => Failure::new(RunExit::BadLaunch, error.to_string()),
        other @ (SecretError::Newer { .. }
        | SecretError::Malformed(_)
        | SecretError::Dpapi { .. }) => Failure::new(RunExit::NoSecret, other.to_string()),
    })?;
    // The agent's window station and desktop (`desktop.rs`), made here so the job can carry the
    // desktop UI limit: no process in it may make or switch desktops. They live as long as this
    // process.
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| {
        Failure::new(
            RunExit::Token,
            format!("the window station's name: random failed ({error})"),
        )
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
        Failure::new(
            RunExit::Token,
            os(&format!("the agent's window station: {call}"), &e),
        )
    })?;
    // The logon, the user's check, the job, the shut, then the resume (`start.rs`); the inner's
    // first act is the sweep of every other process of the pool user's. The job's one handle stays
    // this process's, so the job's processes end with it.
    let mut steps = Start {
        pool_name: &message.pool_name,
        pool_user: &message.pool_user,
        desktop: &desktop,
        logon: &sweeper.logon,
        password: Some(password),
        exe: &sweeper.exe,
        base: set.base(),
        cwd: &sweeper.cwd,
        job,
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
    // then the end sweeper starts.
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
        let failure = Failure::new(RunExit::Pipe, os("ctl write", &error));
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
        Ok(InnerReport::Failed { exit, what }) => return Err(failed(inner, exit, &what)),
        Ok(InnerReport::Exited { .. } | InnerReport::Log { .. } | InnerReport::Escapee { .. }) => {
            return Err(Failure::new(
                RunExit::Pipe,
                "the inner runner skipped its start report",
            ));
        }
        Err(error) => return Err(Failure::new(RunExit::Pipe, os("ctl read", &error))),
    }
    relay(inner, ctl, [stdin, stdout, stderr], end)
}

/// The next report on `ctl` that isn't a `Log` line or an `Escapee` report; each `Log` line
/// goes to stderr, and each `Escapee` report is kept in `end` (the run's outcome) and said.
fn next_report(ctl: &ServerPipe, end: &End) -> io::Result<InnerReport> {
    for _ in 0..LOG_CAP {
        match read_frame::<InnerReport>(ctl)? {
            InnerReport::Log { line } => say(&line),
            InnerReport::Escapee {
                point,
                count,
                images,
            } => {
                end.mark_escapee();
                say(&format!(
                    "clauseo-sandbox-run: sweep ({point}): {count} of the sandbox user's processes \
                     can't be ended ({})",
                    images.join(", ")
                ));
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
        Ok(InnerReport::Failed { exit, what }) => Some(failed(inner, exit, &what)),
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
    Some(Failure::new(
        exit,
        format!("the inner runner exited (0x{code:08X})"),
    ))
}

/// The inner runner's own failure: its `RunExit` code when it is one, after it exited.
fn failed(inner: &Process, exit: u32, what: &str) -> Failure {
    drop(inner.wait(Some(NEXT_CONNECT)));
    Failure::new(RunExit::from_code(exit).unwrap_or(RunExit::Spawn), what)
}

/// Waits for `pipe`'s client and refuses one that isn't the inner runner's process.
fn accept(pipe: &ServerPipe, inner: &Process, timeout: Duration) -> Result<(), Failure> {
    let refused = |what: String| {
        inner.kill(RunExit::Pipe.code());
        Failure::new(RunExit::Pipe, what)
    };
    pipe.connect(inner.handle.0, timeout)
        .map_err(|e| refused(os(pipe.role().suffix(), &e)))?;
    let pid = pipe
        .client_pid()
        .map_err(|e| refused(os("client pid", &e)))?;
    if pid != inner.pid {
        return Err(refused(format!(
            "another process connected the {} pipe",
            pipe.role().suffix()
        )));
    }
    Ok(())
}

/// Relays this process's stdin to the agent's and the agent's stdout and stderr to this
/// process's until the inner runner exits, then exits with its code. The agent's stdin pipe
/// closes when this process's stdin ends, so the agent reads the end of its input, and it has
/// [`GRACE`] to finish before the job is ended (`end::relay_stdin`). `ctl`'s `Log` lines (the
/// inner's sweeps) go to stderr meanwhile. Once the inner exited, the output has [`DRAIN`] per
/// stream to finish; then the job is ended and emptied and the end sweeper started
/// (`end::inner_exited`), which closes what a descendant left in the job held, and the output
/// gets [`DRAIN`] again. On a cancel the exit code is the one the run would have had
/// without the job's end (`end::exit_code`). A sweep of the session that found an escapee makes
/// it [`RunExit::Escapee`] (`EndSweep::outcome`), the code it replaces said on stderr. A clean
/// end (`EndSweep::clean`) removes the session's unclean-session marker.
fn relay(
    inner: &Process,
    ctl: ServerPipe,
    [stdin, stdout, stderr]: [ServerPipe; 3],
    end: &End,
) -> Result<u32, Failure> {
    let spawned = |what: &io::Error| Failure::new(RunExit::Pipe, os("relay thread", what));
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
                    copy(&from, io::stderr());
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
    tell_status(last.as_ref(), code);
    let swept = end.wait();
    if end.clean(swept, reported.is_some())
        && let Err(error) = remove_marker(&end.steps().marker)
    {
        say(&format!(
            "clauseo-sandbox-run: {}",
            os("the session marker's removal", &error)
        ));
    }
    if end.escaped() {
        say(&format!(
            "clauseo-sandbox-run: {ESCAPED}; the run would have exited 0x{code:08X}"
        ));
    }
    exit_now(end.outcome(code))
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
    let which = match report {
        Some(InnerReport::Exited { code: agent }) if *agent >= 0xC000_0000 => {
            format!("the agent ended at Windows status 0x{agent:08X}")
        }
        Some(InnerReport::Exited { .. }) => return,
        _ if code >= 0xC000_0000 => {
            format!("the inner runner ended at Windows status 0x{code:08X}")
        }
        _ => return,
    };
    say(&format!("clauseo-sandbox-run: {which}"));
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

/// Reads the launch file, if the desktop user owns it and only the desktop user, `SYSTEM` and
/// `Administrators` may open it, then removes it.
fn read_launch(path: &Path) -> Result<LaunchMessage, Failure> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !is_launch_name(name) {
        return Err(Failure::new(
            RunExit::Usage,
            "the launch file's name is not a launch file's",
        ));
    }
    let bytes = private_dir::read_unchanged(path, LAUNCH_CAP)
        .map_err(|e| Failure::new(RunExit::BadLaunch, os("launch file", &e)))?
        .ok_or_else(|| {
            Failure::new(
                RunExit::BadLaunch,
                "the launch file is missing or not the desktop user's alone",
            )
        })?;
    if let Err(error) = write::remove_owned_file(path) {
        say(&format!(
            "clauseo-sandbox-run: the launch file stays: {}",
            error.kind()
        ));
    }
    LaunchMessage::parse(&bytes).map_err(|e| Failure::new(RunExit::BadLaunch, e.to_string()))
}

/// `<data_dir>` from `<data_dir>\sandbox\run\<id>.json`.
fn data_dir_of(launch_file: &Path) -> Result<PathBuf, Failure> {
    let run = launch_file.parent();
    let sandbox = run
        .filter(|run| run.file_name().is_some_and(|name| name == RUN_DIR))
        .and_then(Path::parent);
    sandbox
        .filter(|sandbox| sandbox.file_name().is_some_and(|name| name == SANDBOX_DIR))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Failure::new(
                RunExit::Usage,
                "the launch file isn't in <data_dir>\\sandbox\\run",
            )
        })
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
    let refused = |reason: CodexReuse| Failure::new(RunExit::CodexReuse, reason.to_string());
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
        1385 => Failure::new(RunExit::LogonRight, crate::Blocked::LogonRight.to_string()),
        1326 => Failure::new(RunExit::Password, super::PASSWORD_LOST),
        other => Failure::new(
            RunExit::Logon,
            format!("CreateProcessWithLogonW failed ({other})"),
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

/// One line on this process's stderr, which the host's stderr tail shows.
fn say(line: &str) {
    let mut stderr = io::stderr().lock();
    drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
}
