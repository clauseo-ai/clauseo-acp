//! The outer runner (Windows): the desktop user's half, started by the host through
//! `spawn_in_tree` with `--launch <file>`. See `runner.rs` for the whole chain.

use std::io::{self, Write as _};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::desktop::AgentDesktop;
use super::frame::{InnerLaunch, InnerReport, command_line, read_frame, write_frame};
use super::paths::{CodexReuse, WinPath};
use super::spawn::{
    Process, assign_and_resume, end_job, kill_on_close_job, log_on, restrict_dll_search,
    system_folder,
};
use super::{LAUNCH_CAP, LaunchMessage, RUN_DIR, RunExit, agent_env, codex_reuse, is_launch_name};
use crate::windows::acl::object::Handle;
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::pipe::os::ServerPipe;
use crate::windows::pipe::{PipeSet, Role, pipe_sddl};
use crate::windows::record::current_user_sid;
use crate::windows::secrets::{SecretError, secret_for};
use crate::windows::token::os::shut_object;
use crate::windows::token::os::{own_token, profile_folder};
use crate::windows::token::{desktop_sddl, runner_process_sddl, runner_thread_sddl, station_sddl};
use crate::windows::{private_dir, write};

/// How long the inner runner has to connect `ctl`: its logon, which makes the pool user's
/// profile the first time (about 1.7 s on the VM, `s15-spikes-userpool.md`), included.
const FIRST_CONNECT: Duration = Duration::from_secs(60);
/// How long it has for each pipe after that.
const NEXT_CONNECT: Duration = Duration::from_secs(15);
/// How long the agent's output may still drain once the inner runner exited.
const DRAIN: Duration = Duration::from_secs(2);

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
    restrict_dll_search();
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
    let desktop =
        current_user_sid().map_err(|e| Failure::new(RunExit::BadLaunch, os("user", &e)))?;
    let profile = own_profile().map_err(|e| Failure::new(RunExit::BadLaunch, os("profile", &e)))?;
    refuse_codex_reuse(&message, &profile)?;
    let env = agent_env(&message, &profile)
        .map_err(|e| Failure::new(RunExit::BadLaunch, e.to_string()))?;
    let env = super::ca::with_firm_ca(&message, &desktop, env, say);
    let folders = super::env::profile_folders(&message)
        .map_err(|e| Failure::new(RunExit::BadLaunch, e.to_string()))?;
    let set = PipeSet::random().map_err(|e| Failure::new(RunExit::Pipe, os("random", &e)))?;
    let sddl = pipe_sddl(&desktop, &message.pool_user);
    let pipes = Role::ALL
        .iter()
        .map(|role| ServerPipe::create(&set.name(*role), *role, &sddl))
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| Failure::new(RunExit::Pipe, os("CreateNamedPipeW", &e)))?;
    let job = kill_on_close_job().map_err(|e| Failure::new(RunExit::Job, os("job", &e)))?;
    // Only a missing or unusable secret is the set-up's to fix; a read that failed is this
    // launch's alone, and a SID the pool doesn't hold is a bad launch.
    let password = secret_for(&data_dir, &message.pool_user).map_err(|error| match error {
        SecretError::NoSecret { .. } => Failure::new(RunExit::NoSecret, super::PASSWORD_LOST),
        SecretError::Io(_) => Failure::new(RunExit::Logon, error.to_string()),
        SecretError::NotInPool { .. } => Failure::new(RunExit::BadLaunch, error.to_string()),
        other @ (SecretError::Newer { .. }
        | SecretError::Malformed(_)
        | SecretError::Dpapi { .. }) => Failure::new(RunExit::NoSecret, other.to_string()),
    })?;
    let exe_text = exe
        .to_str()
        .ok_or_else(|| Failure::new(RunExit::Usage, "the runner's path isn't UTF-8"))?;
    // The agent's window station and desktop (`desktop.rs`), made here so the job can carry the
    // desktop UI limit: no process in it may make or switch desktops. They live as long as this
    // process.
    let station_name = format!(
        "ClauseoRun-{}",
        set.base()
            .strip_prefix(crate::windows::pipe::PIPE_PREFIX)
            .and_then(|hex| hex.get(..16))
            .unwrap_or("desktop")
    );
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
    // The inner runner starts in System32, a folder its Low agent can't write (no DLL planted in
    // its current folder), and gets the agent's folder over `ctl`.
    let cwd = system_folder().map_err(|e| Failure::new(RunExit::Logon, os("system folder", &e)))?;
    // `log_on` takes the password and wipes it as it returns.
    let inner =
        log_on(&message.pool_name, password, exe_text, set.base(), &cwd).map_err(logon_failure)?;
    // Its token's user, read now: from its first instruction the inner shuts its token to all but
    // SYSTEM until the launch arrives over `ctl`.
    let user = inner
        .user()
        .map_err(|e| Failure::new(RunExit::Logon, os("the inner runner's token", &e)));
    match user {
        Ok(user) if user == message.pool_user => {}
        Ok(_) => {
            inner.kill(RunExit::Logon.code());
            return Err(Failure::new(
                RunExit::Logon,
                "the inner runner isn't the pool user",
            ));
        }
        Err(failure) => {
            inner.kill(RunExit::Logon.code());
            return Err(failure);
        }
    }
    // Shut the inner runner's process and first thread to its agent (the same pool user) while
    // it is still suspended, so no agent of this pool user already running opens either in the
    // moment before the inner shuts itself (`token::os::shut_self`).
    let shut = shut_object(inner.handle.0, &runner_process_sddl(&desktop))
        .and_then(|()| shut_object(inner.thread.0, &runner_thread_sddl(&desktop)));
    if let Err(error) = shut {
        inner.kill(RunExit::Token.code());
        return Err(Failure::new(
            RunExit::Token,
            os("the inner runner's DACL", &error),
        ));
    }
    if let Err(error) = assign_and_resume(&job, &inner) {
        inner.kill(RunExit::Job.code());
        return Err(Failure::new(
            RunExit::Job,
            os("AssignProcessToJobObject", &error),
        ));
    }
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
    let result = serve(&inner, &job, pipes, &launch);
    // `job` drops when this returns: the inner runner and the agent's tree end with it.
    drop(job);
    result
}

/// Connects and checks each pipe, sends the launch, relays the agent's stdio until the inner
/// runner exits, and gives its exit code.
fn serve(
    inner: &Process,
    job: &Handle,
    pipes: Vec<ServerPipe>,
    launch: &InnerLaunch,
) -> Result<u32, Failure> {
    let Ok([ctl, stdin, stdout, stderr]) = <[ServerPipe; 4]>::try_from(pipes) else {
        return Err(Failure::new(RunExit::Pipe, "four pipes expected"));
    };
    // The client is the inner runner's process, whose token user was checked before it ran.
    accept(&ctl, inner, FIRST_CONNECT)?;
    write_frame(&ctl, launch).map_err(|e| Failure::new(RunExit::Pipe, os("ctl write", &e)))?;
    for pipe in [&stdin, &stdout, &stderr] {
        if let Err(failure) = accept(pipe, inner, NEXT_CONNECT) {
            // An inner runner that failed before it opened its pipes said why on `ctl`.
            return Err(reported(&ctl, inner).unwrap_or(failure));
        }
    }
    match read_frame::<InnerReport>(&ctl) {
        Ok(InnerReport::Started { .. }) => {}
        Ok(InnerReport::Failed { exit, what }) => return Err(failed(inner, exit, &what)),
        Ok(InnerReport::Exited { .. }) => {
            return Err(Failure::new(
                RunExit::Pipe,
                "the inner runner skipped its start report",
            ));
        }
        Err(error) => return Err(Failure::new(RunExit::Pipe, os("ctl read", &error))),
    }
    relay(inner, job, &ctl, stdin, stdout, stderr)
}

/// The failure the inner runner reported on `ctl`, if it did.
fn reported(ctl: &ServerPipe, inner: &Process) -> Option<Failure> {
    match read_frame::<InnerReport>(ctl) {
        Ok(InnerReport::Failed { exit, what }) => Some(failed(inner, exit, &what)),
        Ok(InnerReport::Started { .. } | InnerReport::Exited { .. }) | Err(_) => None,
    }
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
/// closes when this process's stdin ends, so the agent reads the end of its input. Once the
/// inner runner exited, the output has [`DRAIN`] to finish; then the job is ended, which closes
/// what a lingering descendant still holds, and the output gets [`DRAIN`] again.
fn relay(
    inner: &Process,
    job: &Handle,
    ctl: &ServerPipe,
    stdin: ServerPipe,
    stdout: ServerPipe,
    stderr: ServerPipe,
) -> Result<u32, Failure> {
    let spawned = |what: &io::Error| Failure::new(RunExit::Pipe, os("relay thread", what));
    // It may stay blocked on the host's pipe; the process ends without it.
    thread::Builder::new()
        .spawn(move || {
            copy(io::stdin().lock(), &stdin);
            drop(stdin);
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
    let code = inner
        .wait(None)
        .ok()
        .flatten()
        .map_or(RunExit::Spawn.code(), super::agent_code);
    let mut left = 2;
    for ended_job in [false, true] {
        if ended_job {
            end_job(job, code);
        }
        while left > 0 && drained.recv_timeout(DRAIN).is_ok() {
            left -= 1;
        }
    }
    tell_status(ctl, code);
    exit_now(code)
}

/// One line on stderr when the run ended at a Windows status (`0xC0000000` and up), naming
/// which process ended so: the agent (its `Exited` report came on `ctl`) or the inner runner.
/// `0xC0000142` (`STATUS_DLL_INIT_FAILED`) at start means a DLL, user32 most often, couldn't
/// start in it: a window station or desktop its token can't open.
fn tell_status(ctl: &ServerPipe, code: u32) {
    let which = match read_frame::<InnerReport>(ctl) {
        Ok(InnerReport::Exited { code: agent }) if agent >= 0xC000_0000 => {
            format!("the agent ended at Windows status 0x{agent:08X}")
        }
        Ok(InnerReport::Exited { .. }) => return,
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

/// Copies `from` to `to` until either end closes, flushing each piece.
fn copy(mut from: impl io::Read, mut to: impl io::Write) {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let Some(piece) = buffer.get(..read) else {
            return;
        };
        if to.write_all(piece).and_then(|()| to.flush()).is_err() {
            return;
        }
    }
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
fn data_dir_of(launch_file: &Path) -> Result<std::path::PathBuf, Failure> {
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
