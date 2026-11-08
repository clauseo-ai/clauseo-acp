//! Starting a program through the runner as a matter's pool user, as the host would: a
//! [`LaunchMessage`] written with [`write_launch`], then `bin\clauseo-sandbox-run.exe --launch
//! <file>` started with piped stdio. [`Running`] reads the agent's stdout and stderr on threads,
//! and every wait has a deadline: a run past it is killed (the outer runner, whose kill-on-close
//! job ends the rest).
//!
//! Every pid the harness starts is in [`STARTED`] until it is waited for, so a row that overruns
//! its limit can have exactly those ended ([`kill_started`]).

use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use clauseo_sandbox::windows::runner::{LAUNCH_VERSION, LaunchMessage, RunExit};
use clauseo_sandbox::windows::runner::{runner_args, write_launch};
use clauseo_sandbox::windows::sids::Sid;

use super::checks_pool::settle;
use super::manifest::{Manifest, Pool, SECOND_A};
use super::rows::Ctx;
use super::sys::{now, sleep, start};
use super::win;

/// The pids the harness started and hasn't waited for.
static STARTED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Notes `pid` as started by the harness.
pub(crate) fn note_started(pid: u32) {
    if let Ok(mut started) = STARTED.lock() {
        started.push(pid);
    }
}

/// Forgets `pid`: it was waited for.
pub(crate) fn note_ended(pid: u32) {
    if let Ok(mut started) = STARTED.lock() {
        started.retain(|p| *p != pid);
    }
}

/// Ends every process the harness started and hasn't waited for; their pids and codes.
pub(crate) fn kill_started() -> Vec<(u32, u32)> {
    let pids: Vec<u32> = STARTED
        .lock()
        .map(|mut started| std::mem::take(&mut *started))
        .unwrap_or_default();
    pids.into_iter()
        .map(|pid| (pid, win::terminate(pid)))
        .collect()
}

/// What to run, and the row's extras.
#[derive(Debug, Clone, Default)]
pub(crate) struct Spec {
    /// The program, absolute.
    pub(crate) program: String,
    /// Its arguments.
    pub(crate) args: Vec<String>,
    /// Variables beyond the minimal block.
    pub(crate) env: Vec<(String, String)>,
    /// Folders put before System32 on `PATH`, and added to the runtime paths.
    pub(crate) path: Vec<PathBuf>,
    /// The CA bundle, when the row uses one.
    pub(crate) ca_bundle: Option<PathBuf>,
    /// The credential link, when the row names one (the `m3-hardlink` row); granted too, as
    /// `granted_paths` grants a policy's.
    pub(crate) credential_link: Option<PathBuf>,
}

impl Spec {
    /// The harness's own probe `args` (after `probe`).
    pub(crate) fn probe(manifest: &Manifest, args: &[&str]) -> Spec {
        let mut all = vec!["probe".to_owned()];
        all.extend(args.iter().map(|arg| (*arg).to_owned()));
        Spec {
            program: text(&manifest.harness_exe()),
            args: all,
            ..Spec::default()
        }
    }
}

/// `path` as the message's text.
pub(crate) fn text(path: &Path) -> String {
    path.display().to_string()
}

/// Session key `which`'s pool user: `A` and `B` their matters', [`SECOND_A`] matter A's second.
pub(crate) fn pool_of<'a>(manifest: &'a Manifest, which: &str) -> Result<&'a Pool, String> {
    if which == SECOND_A {
        return manifest.second_a().ok_or_else(|| {
            "the manifest names no third pool user: run cleanup, then prepare".into()
        });
    }
    let (a, b) = manifest
        .pool_pair()
        .ok_or("the manifest names no two pool users: run prepare")?;
    Ok(if which == "A" { a } else { b })
}

/// Matter `which`'s ID, fixed for the harness: the outer's git watch sets a new `.git` aside into
/// its set-aside. `SECOND_A` is matter A's second session.
fn matter_id(which: &str) -> Result<clauseo_protocol::MatterId, String> {
    let id = if which == "B" {
        "01890a5d-ac96-774b-bcce-b302099a8b02"
    } else {
        "01890a5d-ac96-774b-bcce-b302099a8a01"
    };
    id.parse().map_err(|_bad| format!("bad matter id {id}"))
}

/// The launch message for `spec` as matter `which`'s pool user.
pub(crate) fn message(
    manifest: &Manifest,
    which: &str,
    spec: &Spec,
) -> Result<LaunchMessage, String> {
    let pool = pool_of(manifest, which)?;
    let matter = text(&manifest.matter(which));
    let state = text(&manifest.state(which));
    let bin = text(&manifest.bin());
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_unset| r"C:\Windows".to_owned());
    let mut path: Vec<String> = spec.path.iter().map(|p| text(p)).collect();
    path.extend([
        format!(r"{system_root}\System32"),
        system_root.clone(),
        format!(r"{system_root}\System32\Wbem"),
        format!(r"{system_root}\System32\WindowsPowerShell\v1.0"),
    ]);
    let mut env = vec![
        ("SystemRoot".to_owned(), system_root.clone()),
        ("windir".to_owned(), system_root.clone()),
        ("PATH".to_owned(), path.join(";")),
        (
            "ComSpec".to_owned(),
            format!(r"{system_root}\System32\cmd.exe"),
        ),
    ];
    for name in [
        "PATHEXT",
        "SystemDrive",
        "PROCESSOR_ARCHITECTURE",
        "NUMBER_OF_PROCESSORS",
        "OS",
    ] {
        if let Ok(value) = std::env::var(name) {
            env.push((name.to_owned(), value));
        }
    }
    env.extend(spec.env.iter().cloned());
    let mut runtime = vec![bin];
    runtime.extend(spec.path.iter().map(|p| text(p)));
    let link = spec.credential_link.as_deref().map(text);
    let message = LaunchMessage {
        version: LAUNCH_VERSION,
        pool_user: Sid::parse(&pool.sid).map_err(|e| format!("pool SID: {e}"))?,
        pool_name: pool.name.clone(),
        matter_sid: None,
        matter: matter_id(which)?,
        matter_root: matter.clone(),
        notes_root: None,
        program: spec.program.clone(),
        args: spec.args.clone(),
        cwd: matter.clone(),
        env,
        state: state.clone(),
        tmpdir: None,
        runtime: runtime.clone(),
        granted: [
            vec![matter, state],
            runtime,
            link.clone().into_iter().collect(),
        ]
        .concat(),
        ca_bundle: spec.ca_bundle.as_deref().map(text),
        credential_link: link,
        firm_ca: Vec::new(),
        denial_pipe: None,
    };
    message.check().map_err(|e| e.to_string())?;
    Ok(message)
}

/// One line from the runner: stdout or stderr.
enum Line {
    Out(String),
    Err(String),
}

/// A run in progress.
pub(crate) struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
    /// Every stdout line read so far.
    pub(crate) stdout: Vec<String>,
    /// Every stderr line read so far.
    pub(crate) stderr: Vec<String>,
    started: Instant,
    /// Milliseconds from the runner's start to the agent's first stdout line: the logon (and
    /// the first profile) dominate it.
    pub(crate) first_line_ms: Option<u128>,
    waited: bool,
}

/// A finished run.
#[derive(Debug)]
pub(crate) struct Finished {
    /// The runner's exit code.
    pub(crate) code: u32,
    /// Its stdout lines.
    pub(crate) stdout: Vec<String>,
    /// Its stderr lines.
    pub(crate) stderr: Vec<String>,
    /// Wall time from the runner's start to its exit.
    pub(crate) wall_ms: u128,
    /// Milliseconds to the first stdout line.
    pub(crate) first_line_ms: Option<u128>,
}

/// Starts `spec` through the runner as session key `which`'s pool user, once no process of
/// that user is left (`checks_pool::settle`): its last session has ended, and nothing escaped it.
pub(crate) fn launch(ctx: &Ctx<'_>, which: &str, spec: &Spec) -> Result<Running, String> {
    settle(ctx, pool_of(ctx.manifest, which)?)?;
    launch_now(ctx.manifest, which, spec)
}

/// [`launch`] without the wait: for `sweep-before-logon`, whose own stray process of the pool
/// user must still run when the session starts.
pub(crate) fn launch_now(manifest: &Manifest, which: &str, spec: &Spec) -> Result<Running, String> {
    let message = message(manifest, which, spec)?;
    let file = write_launch(&manifest.data_dir, &message)
        .map_err(|e| format!("write_launch failed: {e}"))?;
    let mut command = Command::new(manifest.runner_exe());
    command
        .args(runner_args(&file))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_command(&mut command)
}

/// Starts `command` (stdio piped) and reads it as a [`Running`].
pub(crate) fn run_command(command: &mut Command) -> Result<Running, String> {
    let started = now();
    let mut child = start(command).map_err(|e| format!("starting the runner failed: {e}"))?;
    note_started(child.id());
    let (send, lines) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        read_lines(out, send.clone(), Line::Out);
    }
    if let Some(err) = child.stderr.take() {
        read_lines(err, send, Line::Err);
    }
    Ok(Running {
        stdin: child.stdin.take(),
        child,
        lines,
        stdout: Vec::new(),
        stderr: Vec::new(),
        started,
        first_line_ms: None,
        waited: false,
    })
}

fn read_lines(from: impl Read + Send + 'static, to: Sender<Line>, wrap: fn(String) -> Line) {
    std::thread::spawn(move || {
        for line in BufReader::new(from).lines() {
            let Ok(line) = line else { return };
            if to.send(wrap(line.trim_end().to_owned())).is_err() {
                return;
            }
        }
    });
}

impl Running {
    /// The outer runner's pid.
    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Takes one line from the channel, waiting until `until`; `false` when none came.
    fn pump(&mut self, until: Instant) -> bool {
        let left = until.saturating_duration_since(now());
        match self.lines.recv_timeout(left) {
            Ok(Line::Out(line)) => {
                if self.first_line_ms.is_none() {
                    self.first_line_ms = Some(self.started.elapsed().as_millis());
                }
                self.stdout.push(line);
                true
            }
            Ok(Line::Err(line)) => {
                self.stderr.push(line);
                true
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => false,
        }
    }

    /// The first stdout line starting with `prefix` (read so far or coming before `until`),
    /// without it.
    pub(crate) fn line_after(&mut self, prefix: &str, until: Instant) -> Option<String> {
        let mut from = 0;
        loop {
            let found = self
                .stdout
                .get(from..)
                .unwrap_or_default()
                .iter()
                .find_map(|line| line.strip_prefix(prefix).map(str::to_owned));
            if found.is_some() {
                return found;
            }
            from = self.stdout.len();
            if now() >= until || !self.pump(until) {
                return None;
            }
        }
    }

    /// Writes `text` to the agent's stdin.
    pub(crate) fn send(&mut self, text: &str) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("stdin is closed")?;
        stdin
            .write_all(text.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|e| format!("writing stdin failed: {e}"))
    }

    /// Closes the agent's stdin.
    pub(crate) fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    /// Takes the agent's stdin, for a row that writes and closes it from a thread of its own.
    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    /// Whether the outer runner exited by itself before `until`, its output read meanwhile.
    /// Once it has, it counts as waited for; still running, it is left as it is.
    pub(crate) fn exited_by(&mut self, until: Instant) -> bool {
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                note_ended(self.child.id());
                self.waited = true;
                return true;
            }
            if now() >= until {
                return false;
            }
            while self.pump(now()) {}
            sleep(Duration::from_millis(25));
        }
    }

    /// Ends the outer runner now (as the host's job kill would) and waits for it.
    pub(crate) fn kill(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
        note_ended(self.child.id());
        self.waited = true;
    }

    /// Reads the runner's output until its pipes close or `until` (after [`Running::kill`]).
    pub(crate) fn drain(&mut self, until: Instant) {
        while self.pump(until) {}
    }

    /// Waits for the agent to end by itself, until `until`: its stdin stays open until the
    /// runner exited. The host closes stdin only to end a session, and the runner ends the job
    /// 1 s after that, so an agent still working would be cut short. Killed and `Err` when it
    /// is still running at `until`.
    pub(crate) fn finish(self, until: Instant) -> Result<Finished, String> {
        self.wait_exit(until)
    }

    /// Ends the session as the host does: closes the agent's stdin, then waits as
    /// [`Running::finish`] does. For the agents that end at their stdin's end (`hold`, `echo`).
    pub(crate) fn end(mut self, until: Instant) -> Result<Finished, String> {
        self.close_stdin();
        self.wait_exit(until)
    }

    /// Waits for the runner to exit until `until`, reading its output, then closes stdin;
    /// killed and `Err` when it is still running then.
    fn wait_exit(mut self, until: Instant) -> Result<Finished, String> {
        let code = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    break u32::from_ne_bytes(code.to_ne_bytes());
                }
                Ok(None) => {}
                Err(e) => return Err(format!("waiting for the runner failed: {e}")),
            }
            if now() >= until {
                let after = self.started.elapsed().as_secs();
                self.kill();
                return Err(format!(
                    "timed out after {after} s; stdout {:?}; stderr {:?}",
                    self.stdout, self.stderr
                ));
            }
            while self.pump(now()) {}
            sleep(Duration::from_millis(50));
        };
        note_ended(self.child.id());
        self.waited = true;
        self.close_stdin();
        let wall_ms = self.started.elapsed().as_millis();
        // The output drains once the runner's pipes close; two seconds at most.
        let drain = now() + Duration::from_secs(2);
        while self.pump(drain) {}
        Ok(Finished {
            code,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            wall_ms,
            first_line_ms: self.first_line_ms,
        })
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.waited {
            self.kill();
        }
    }
}

/// `code` as the FAIL line names it: a [`RunExit`] by name, else the number.
pub(crate) fn describe(code: u32) -> String {
    match RunExit::from_code(code) {
        Some(exit) => format!("{code:#x} (RunExit::{exit:?})"),
        None => code.to_string(),
    }
}

/// One line about a finished run: its exit, its wall time and its time to the first line.
pub(crate) fn summary(done: &Finished) -> String {
    let first = done
        .first_line_ms
        .map_or_else(|| "no line".to_owned(), |ms| format!("{ms} ms"));
    format!(
        "exit {}, wall {} ms, first line after {first} (logon included)",
        describe(done.code),
        done.wall_ms
    )
}

/// The deadline `limit` from now.
pub(crate) fn deadline(limit: Duration) -> Instant {
    now() + limit
}
