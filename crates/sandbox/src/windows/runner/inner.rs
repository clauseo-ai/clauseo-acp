//! The inner runner (Windows): the pool user's half, started by the outer runner through
//! `CreateProcessWithLogonW` with `--inner <pipe base> <desktop user's SID>`. See `runner.rs`
//! for the whole chain and `inner_flow.rs` for its order.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use super::RunExit;
use super::frame::{InnerGo, InnerLaunch, InnerReady, InnerReport, read_frame, write_frame};
use super::inner_flow::{Failure, Inner, run};
use super::leftovers::os::OsLeftovers;
use super::leftovers::{Kind, remove_all};
use super::spawn::{Agent, Process, quiet_errors, restrict_dll_search, start_agent};
use super::station::StationDesktop;
use super::sweep::os::sweep_own;
use super::sweep::{Point, SweepError};
use crate::windows::acl::object::Handle;
use crate::windows::pipe::os::{into_file, open_client, wait_readable};
use crate::windows::pipe::{PipeSet, Role};
use crate::windows::sids::Sid;
use crate::windows::token::TokenSpec;
use crate::windows::token::os::{
    logon_sid, make, own_token, shut_inner_token_first, shut_later, shut_self, user,
};
use crate::windows::write;

/// How long the inner waits for the outer's go after its ready: twice the outer's own wait for
/// the ready (`inner_user::READY_WAIT`), which the full check and the go follow at once.
const GO_WAIT: Duration = Duration::from_secs(60);

/// The inner runner's whole run; its exit code: the agent's, or a [`RunExit`] code.
pub fn run_inner(base: &str, desktop_user: &Sid) -> u32 {
    if restrict_dll_search().is_err() {
        return RunExit::DllSearch.code();
    }
    quiet_errors();
    // First of all: shut this runner's own token to all but SYSTEM and the desktop user's
    // TOKEN_QUERY (`token::runner_token_first_sddl`), so the outer can open it for its full
    // check before its go (`inner_flow.rs`, step 0). The outer read its user while it was
    // suspended. The handle opened here keeps its rights. A grant that fails refuses the launch.
    let Ok(token) = own_token() else {
        return RunExit::TokenGrant.code();
    };
    if shut_inner_token_first(&token, desktop_user).is_err() {
        return RunExit::TokenGrant.code();
    }
    let Ok(me) = user(&token) else {
        return RunExit::Token.code();
    };
    let Ok(set) = PipeSet::parse(base) else {
        return RunExit::Usage.code();
    };
    let mut calls = Calls {
        set,
        token,
        me,
        held: Vec::new(),
        opened: None,
        ctl: None,
        launch: None,
    };
    run(&mut calls)
}

/// The inner's calls.
struct Calls {
    set: PipeSet,
    token: Handle,
    /// This runner's user: the pool user, as the outer checked.
    me: Sid,
    /// Lines and reports told before `connect`.
    held: Vec<InnerReport>,
    /// `ctl` once the handshake opened it, until `connect` takes it: lines stay held meanwhile,
    /// as before the handshake, so nothing is written that the outer doesn't read yet.
    opened: Option<File>,
    ctl: Option<File>,
    launch: Option<InnerLaunch>,
}

impl Inner for Calls {
    type Agent = Process;

    fn sweep(&mut self, point: Point) -> Result<(), SweepError> {
        let mut lines = Vec::new();
        let swept = sweep_own(&self.me, point, &mut |line| {
            lines.push(line.to_owned());
        });
        for line in lines {
            self.log(&line);
        }
        swept
    }

    fn clean(&mut self) {
        let mut lines = Vec::new();
        let mut calls = OsLeftovers::new(&self.token);
        remove_all(&mut calls, &Kind::LOCAL, "before launch", &mut |line| {
            lines.push(line.to_owned());
        });
        drop(calls);
        for line in lines {
            self.log(&line);
        }
    }

    fn log(&mut self, line: &str) {
        self.tell(InnerReport::Log {
            line: line.to_owned(),
        });
    }

    fn tell(&mut self, report: InnerReport) {
        match &self.ctl {
            // A report that can't be sent is lost (the outer is gone); the run goes on.
            Some(ctl) => drop(write_frame(ctl, &report)),
            None => self.held.push(report),
        }
    }

    fn handshake(&mut self) -> Result<(), RunExit> {
        let Ok(ctl) = open_client(&self.set.name(Role::Ctl), Role::Ctl, false) else {
            return Err(RunExit::Pipe);
        };
        let ctl = into_file(ctl);
        write_frame(
            &ctl,
            &InnerReady {
                pid: std::process::id(),
            },
        )
        .map_err(|_e| RunExit::Pipe)?;
        // Waits for the outer's full check; a failed one kills this process. Bounded by
        // [`GO_WAIT`], on this thread alone (`wait_readable`): no thread is made before the
        // sweep, and every thread this runner has is shut at the launch (`shut_self`).
        wait_readable(&ctl, GO_WAIT).map_err(|_e| RunExit::Pipe)?;
        match read_frame::<InnerGo>(&ctl) {
            Ok(InnerGo { go: true }) => {}
            Ok(InnerGo { go: false }) | Err(_) => return Err(RunExit::Pipe),
        }
        self.opened = Some(ctl);
        Ok(())
    }

    fn connect(&mut self) -> Result<(), RunExit> {
        let Some(ctl) = self.opened.take() else {
            return Err(RunExit::Pipe);
        };
        self.ctl = Some(ctl);
        for report in std::mem::take(&mut self.held) {
            self.tell(report);
        }
        Ok(())
    }

    fn read_launch(&mut self) -> Result<(), Failure> {
        let ctl = self
            .ctl
            .as_ref()
            .ok_or_else(|| (RunExit::Pipe, "ctl isn't open".to_owned()))?;
        let launch: InnerLaunch =
            read_frame(ctl).map_err(|e| fail(RunExit::Pipe, "ctl read", &e))?;
        // The desktop must be the outer's own station's `Default`, never `WinSta0` nor empty
        // (`StationDesktop`): the agent starts on it.
        if StationDesktop::from_path(&launch.desktop).is_none() {
            return Err((
                RunExit::Token,
                "the launch's desktop isn't a station's Default".to_owned(),
            ));
        }
        // Then, before anything else: shut this runner's token (now with the desktop user's
        // query), process, threads and later objects to the agent (`token::os::shut_self`); the
        // outer shut the process and first thread while this runner was still suspended.
        let logon = logon_sid(&self.token).map_err(|e| fail(RunExit::Token, "logon SID", &e))?;
        shut_self(&self.token, &launch.desktop_user, &logon)
            .map_err(|e| fail(RunExit::Token, "the inner runner's own DACL", &e))?;
        self.launch = Some(launch);
        Ok(())
    }

    fn start(&mut self) -> Result<Process, Failure> {
        let launch = self
            .launch
            .as_ref()
            .ok_or_else(|| (RunExit::Pipe, "no launch".to_owned()))?;
        start(&self.set, launch, &self.token)
    }

    fn pid(&self, agent: &Process) -> u32 {
        agent.pid
    }

    fn wait(&mut self, agent: &Process, interval: Duration) -> Option<u32> {
        match agent.wait(Some(interval)) {
            Ok(None) => None,
            Ok(Some(code)) => Some(code),
            Err(_) => Some(RunExit::Spawn.code()),
        }
    }

    fn kill(&mut self, agent: &Process, code: u32) {
        agent.kill(code);
    }

    fn report(&mut self, report: &InnerReport) -> bool {
        self.ctl
            .as_ref()
            .is_some_and(|ctl| write_frame(ctl, report).is_ok())
    }
}

/// Makes the folders and the token, opens the agent's pipes, and starts it on the desktop the
/// outer runner made.
fn start(set: &PipeSet, launch: &InnerLaunch, token: &Handle) -> Result<Process, Failure> {
    let me = user(token).map_err(|e| fail(RunExit::Token, "token user", &e))?;
    if me != launch.pool_user {
        return Err((
            RunExit::Token,
            "the inner runner isn't the pool user".to_owned(),
        ));
    }
    let logon = logon_sid(token).map_err(|e| fail(RunExit::Token, "logon SID", &e))?;
    let spec = TokenSpec::for_pool(
        &me,
        &logon,
        &launch.desktop_user,
        launch.matter_sid.as_ref(),
    )
    .map_err(|e| (RunExit::Token, e.to_string()))?;
    for folder in &launch.folders {
        write::make_agent_folder(Path::new(&launch.folder_base), Path::new(folder))
            .map_err(|e| fail(RunExit::Token, "profile folder", &e))?;
    }
    let stdio = [Role::In, Role::Out, Role::Err]
        .iter()
        .map(|role| open_client(&set.name(*role), *role, true))
        .collect::<std::io::Result<Vec<Handle>>>()
        .map_err(|e| fail(RunExit::Pipe, "open pipe", &e))?;
    let [stdin, stdout, stderr] = stdio.as_slice() else {
        return Err((RunExit::Pipe, "three pipes expected".to_owned()));
    };
    let agent_token =
        make(token, &spec).map_err(|e| fail(RunExit::Token, "restricted token", &e))?;
    shut_later(token, &launch.desktop_user)
        .map_err(|e| fail(RunExit::Token, "the inner runner's default DACL", &e))?;
    let agent = start_agent(&Agent {
        token: &agent_token,
        desktop: &launch.desktop,
        program: &launch.program,
        command_line: &launch.command_line,
        cwd: &launch.cwd,
        env: &launch.env,
        stdio: [stdin.0, stdout.0, stderr.0],
    })
    .map_err(|e| fail(RunExit::Spawn, "CreateProcessAsUserW", &e))?;
    // This runner's copies of the pipes close here, so the agent's exit ends the streams.
    drop(stdio);
    Ok(agent)
}

fn fail(exit: RunExit, call: &str, error: &std::io::Error) -> Failure {
    let code = error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string());
    (exit, format!("{call} failed ({code})"))
}
