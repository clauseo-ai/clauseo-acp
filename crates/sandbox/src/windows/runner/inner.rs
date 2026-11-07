//! The inner runner (Windows): the pool user's half, started by the outer runner through
//! `CreateProcessWithLogonW` with `--inner <pipe base>`. See `runner.rs` for the whole chain.
#![expect(unsafe_code, reason = "a pipe handle handed to std as a File")]

use std::fs::File;
use std::mem::ManuallyDrop;
use std::os::windows::io::FromRawHandle as _;
use std::path::Path;

use super::RunExit;
use super::frame::{InnerLaunch, InnerReport, read_frame, write_frame};
use super::spawn::{Agent, Desktop, quiet_errors, start_agent};
use crate::windows::acl::object::Handle;
use crate::windows::pipe::os::open_client;
use crate::windows::pipe::{PipeSet, Role};
use crate::windows::token::os::{logon_sid, make, own_token, user};
use crate::windows::token::{TokenSpec, desktop_sddl};
use crate::windows::write;

/// A failure before the agent ran: its exit code and what failed (a call and its code).
struct Failure(RunExit, String);

/// The inner runner's whole run; its exit code: the agent's, or a [`RunExit`] code.
pub fn run_inner(base: &str) -> u32 {
    quiet_errors();
    let Ok(set) = PipeSet::parse(base) else {
        return RunExit::Usage.code();
    };
    let Ok(ctl) = open_client(&set.name(Role::Ctl), Role::Ctl, false) else {
        return RunExit::Pipe.code();
    };
    let ctl = into_file(ctl);
    let launch: InnerLaunch = match read_frame(&ctl) {
        Ok(launch) => launch,
        Err(_) => return RunExit::Pipe.code(),
    };
    match start(&set, &launch) {
        Ok((agent, desktop)) => {
            if write_frame(&ctl, &InnerReport::Started { pid: agent.pid }).is_err() {
                agent.kill(RunExit::Pipe.code());
            }
            drop(ctl);
            let code = agent
                .wait(None)
                .ok()
                .flatten()
                .unwrap_or(RunExit::Spawn.code());
            drop(desktop);
            code
        }
        Err(Failure(exit, what)) => {
            // The outer may be gone already; the exit code says it anyway.
            drop(write_frame(
                &ctl,
                &InnerReport::Failed {
                    exit: exit.code(),
                    what,
                },
            ));
            exit.code()
        }
    }
}

/// Makes the folders, the token and the desktop, opens the agent's pipes, and starts it.
fn start(set: &PipeSet, launch: &InnerLaunch) -> Result<(super::spawn::Process, Desktop), Failure> {
    let token = own_token().map_err(|e| fail(RunExit::Token, "OpenProcessToken", &e))?;
    let me = user(&token).map_err(|e| fail(RunExit::Token, "token user", &e))?;
    if me != launch.pool_user {
        return Err(Failure(
            RunExit::Token,
            "the inner runner isn't the pool user".to_owned(),
        ));
    }
    let logon = logon_sid(&token).map_err(|e| fail(RunExit::Token, "logon SID", &e))?;
    let spec = TokenSpec::for_pool(
        &me,
        &logon,
        &launch.desktop_user,
        launch.matter_sid.as_ref(),
    )
    .map_err(|e| Failure(RunExit::Token, e.to_string()))?;
    for folder in &launch.folders {
        write::make_agent_folder(Path::new(folder))
            .map_err(|e| fail(RunExit::Token, "profile folder", &e))?;
    }
    let stdio = [Role::In, Role::Out, Role::Err]
        .iter()
        .map(|role| open_client(&set.name(*role), *role, true))
        .collect::<std::io::Result<Vec<Handle>>>()
        .map_err(|e| fail(RunExit::Pipe, "open pipe", &e))?;
    let [stdin, stdout, stderr] = stdio.as_slice() else {
        return Err(Failure(RunExit::Pipe, "three pipes expected".to_owned()));
    };
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| Failure(RunExit::Token, e.to_string()))?;
    let name = format!("clauseo-{}", crate::windows::pipe::hex(&random));
    let desktop = Desktop::create(&name, &desktop_sddl(&me, launch.matter_sid.as_ref()))
        .map_err(|e| fail(RunExit::Token, "CreateDesktopW", &e))?;
    let agent_token =
        make(&token, &spec).map_err(|e| fail(RunExit::Token, "restricted token", &e))?;
    let agent = start_agent(&Agent {
        token: &agent_token,
        desktop: &desktop.path,
        program: &launch.program,
        command_line: &launch.command_line,
        cwd: &launch.cwd,
        env: &launch.env,
        stdio: [stdin.0, stdout.0, stderr.0],
    })
    .map_err(|e| fail(RunExit::Spawn, "CreateProcessAsUserW", &e))?;
    // This runner's copies of the pipes close here, so the agent's exit ends the streams.
    drop(stdio);
    Ok((agent, desktop))
}

fn fail(exit: RunExit, call: &str, error: &std::io::Error) -> Failure {
    let code = error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string());
    Failure(exit, format!("{call} failed ({code})"))
}

/// `handle` as a `File`, which owns and closes it from now on.
fn into_file(handle: Handle) -> File {
    let handle = ManuallyDrop::new(handle);
    // SAFETY: the handle is live and owned; `ManuallyDrop` keeps `Handle` from closing it, so the
    // `File` is its one owner.
    unsafe { File::from_raw_handle(handle.0.0) }
}
