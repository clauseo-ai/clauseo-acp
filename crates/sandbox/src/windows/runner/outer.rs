//! The outer runner (Windows): the desktop user's half, started by the host through
//! `spawn_in_tree` with `--launch <file>`. See `runner.rs` for the whole chain.
#![expect(unsafe_code, reason = "FFI to the profile folder and token calls")]

use std::io::{self, Write as _};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::System::Threading::OpenProcessToken;
use windows::Win32::UI::Shell::GetUserProfileDirectoryW;
use windows::core::PWSTR;

use super::frame::{InnerLaunch, InnerReport, command_line, read_frame, write_frame};
use super::paths::{CodexReuse, WinPath};
use super::spawn::{Process, assign_and_resume, kill_on_close_job, log_on};
use super::{LAUNCH_CAP, LaunchMessage, RUN_DIR, RunExit, agent_env, codex_reuse, is_launch_name};
use crate::windows::acl::object::{Handle, Object, os_error};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::pipe::os::ServerPipe;
use crate::windows::pipe::{PipeSet, Role, pipe_sddl};
use crate::windows::record::{current_user_sid, user_of};
use crate::windows::secrets::{SecretError, secret_for};
use crate::windows::token::os::own_token;
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
    let password = secret_for(&data_dir, &message.pool_user).map_err(|error| match error {
        SecretError::NoSecret { .. } => Failure::new(RunExit::NoSecret, super::PASSWORD_LOST),
        other @ (SecretError::Newer { .. }
        | SecretError::Malformed(_)
        | SecretError::Io(_)
        | SecretError::Dpapi { .. }
        | SecretError::NotInPool { .. }) => Failure::new(RunExit::NoSecret, other.to_string()),
    })?;
    let exe_text = exe
        .to_str()
        .ok_or_else(|| Failure::new(RunExit::Usage, "the runner's path isn't UTF-8"))?;
    let state = WinPath::parse(&message.state)
        .map(|p| p.display())
        .unwrap_or_default();
    // `log_on` takes the password and wipes it as it returns.
    let inner = log_on(&message.pool_name, password, exe_text, set.base(), &state)
        .map_err(logon_failure)?;
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
    };
    let result = serve(&inner, &message, &pipes, &launch);
    // `job` drops when this returns: the inner runner and the agent's tree end with it.
    drop(job);
    result
}

/// Connects and checks each pipe, sends the launch, relays the agent's stdio until the inner
/// runner exits, and gives its exit code.
fn serve(
    inner: &Process,
    message: &LaunchMessage,
    pipes: &[ServerPipe],
    launch: &InnerLaunch,
) -> Result<u32, Failure> {
    let [ctl, stdin, stdout, stderr] = pipes else {
        return Err(Failure::new(RunExit::Pipe, "four pipes expected"));
    };
    accept(ctl, inner, FIRST_CONNECT)?;
    let user = process_user(inner).map_err(|e| Failure::new(RunExit::Pipe, os("token", &e)))?;
    if user != message.pool_user {
        inner.kill(RunExit::Pipe.code());
        return Err(Failure::new(
            RunExit::Pipe,
            "the inner runner isn't the pool user",
        ));
    }
    write_frame(ctl, launch).map_err(|e| Failure::new(RunExit::Pipe, os("ctl write", &e)))?;
    for pipe in [stdin, stdout, stderr] {
        accept(pipe, inner, NEXT_CONNECT)?;
    }
    match read_frame::<InnerReport>(ctl) {
        Ok(InnerReport::Started { .. }) => {}
        Ok(InnerReport::Failed { exit, what }) => {
            say(&format!("clauseo-sandbox-run: {what}"));
            return Ok(inner
                .wait(Some(NEXT_CONNECT))
                .ok()
                .flatten()
                .unwrap_or(exit));
        }
        Err(error) => return Err(Failure::new(RunExit::Pipe, os("ctl read", &error))),
    }
    Ok(relay(inner, stdin, stdout, stderr))
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
/// process's, until the inner runner exits; its exit code.
fn relay(inner: &Process, stdin: &ServerPipe, stdout: &ServerPipe, stderr: &ServerPipe) -> u32 {
    thread::scope(|scope| {
        // The stdin copy may stay blocked on the host's pipe; the process ends without it.
        scope.spawn(|| copy(io::stdin().lock(), stdin));
        let (done, drained) = mpsc::channel();
        for (from, to) in [(stdout, 1u8), (stderr, 2u8)] {
            let done = done.clone();
            scope.spawn(move || {
                match to {
                    1 => copy(from, io::stdout().lock()),
                    _ => copy(from, io::stderr().lock()),
                }
                let _sent = done.send(());
            });
        }
        drop(done);
        let code = inner
            .wait(None)
            .ok()
            .flatten()
            .unwrap_or(RunExit::Spawn.code());
        for _ in 0..2 {
            if drained.recv_timeout(DRAIN).is_err() {
                break;
            }
        }
        // The scope would join the copies; leave instead, which ends the job's processes.
        exit_now(code)
    })
}

/// Ends this process with `code` now: the copies still blocked on a pipe go with it, and the
/// kill-on-close job ends whatever of the agent's tree is left.
#[expect(
    clippy::exit,
    reason = "the copy threads may block on pipes; only an exit ends them"
)]
fn exit_now(code: u32) -> ! {
    drop(io::stdout().flush());
    // Windows takes the exit code as the u32 it is; `as` keeps its bits.
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
    let refused = |reason: CodexReuse| Failure::new(RunExit::CodexReuse, reason.to_string());
    codex_reuse(profile, paths, &message.env).map_err(refused)?;
    let Some(link) = &message.credential_link else {
        return Ok(());
    };
    let auth = profile.join(".codex").join("auth.json").display();
    let id = |path: &str| match Object::open(Path::new(path)) {
        Ok(object) => object.file_id().map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    let checked = id(&auth).and_then(|auth| Ok((auth, id(link)?)));
    match checked {
        Ok((Some(auth), Some(link))) if auth == link => Err(refused(CodexReuse::LinkedLogin)),
        Ok(_) => Ok(()),
        Err(error) => Err(Failure::new(
            RunExit::CodexReuse,
            os("credential link check", &error),
        )),
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

/// The user of `process`'s token.
fn process_user(process: &Process) -> io::Result<crate::windows::sids::Sid> {
    let mut token = HANDLE::default();
    // SAFETY: the process handle has every right (it came from the logon call); `token` is a
    // live out-pointer, owned by `Handle` once written.
    unsafe { OpenProcessToken(process.handle.0, TOKEN_QUERY, &raw mut token) }
        .map_err(|e| os_error(&e))?;
    let token = Handle(token);
    let text = user_of(token.0)?;
    crate::windows::sids::Sid::parse(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// This user's profile folder, from its token (never from `USERPROFILE`).
fn own_profile() -> io::Result<WinPath> {
    let token = own_token()?;
    let mut buffer = vec![0u16; 1024];
    let mut len = u32::try_from(buffer.len()).map_err(io::Error::other)?;
    // SAFETY: the token is live; `buffer` is writable for `len` units.
    unsafe { GetUserProfileDirectoryW(token.0, Some(PWSTR(buffer.as_mut_ptr())), &raw mut len) }
        .map_err(|e| os_error(&e))?;
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let text =
        String::from_utf16(buffer.get(..end).unwrap_or_default()).map_err(io::Error::other)?;
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
