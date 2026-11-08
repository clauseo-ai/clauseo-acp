//! The fake outer's side of the world: what `spawn_outer` was given, whether it starts, how
//! long the pool user's processes linger, and the fake denial pipe (the outer it can open, its
//! client, and what the client writes).

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::super::feed::Got;
use super::super::{DenialPipe, Peer};

/// The outer's side of the world.
#[derive(Default)]
pub(super) struct Outer {
    pub(super) lingering: Mutex<u32>,
    pub(super) spawned: Mutex<Option<Spawned>>,
    pub(super) denial_pipe: Mutex<Option<String>>,
    pub(super) spawn_starts: Mutex<bool>,
    pub(super) pipe_connects: Mutex<bool>,
    /// The pids the launchers' pipes opened as the outer, in order.
    pub(super) watched: std::sync::Arc<Mutex<Vec<u32>>>,
}

/// What `spawn_outer` was given.
#[derive(Debug, Clone)]
pub(super) struct Spawned {
    pub(super) program: PathBuf,
    pub(super) args: Vec<OsString>,
    pub(super) env: BTreeMap<OsString, OsString>,
    pub(super) cwd: PathBuf,
    pub(super) grace: Option<Duration>,
    pub(super) clears_env: bool,
}

/// The fake's install folder: one that exists, as a spec's working folder must.
pub(super) fn install() -> PathBuf {
    std::env::temp_dir()
}

/// The host's variables, as the fake reads them.
pub(super) fn host_var(name: &str) -> Option<OsString> {
    match name {
        "SystemRoot" | "windir" => Some(r"C:\Windows".into()),
        "TEMP" => Some(r"C:\Users\lawyer\AppData\Local\Temp".into()),
        "PATH" | "USERPROFILE" => Some("not for the outer".into()),
        _ => None,
    }
}

impl Spawned {
    /// What `spawn_outer` was given: `spec` and `opts`.
    pub(super) fn of(
        spec: &clauseo_process::LaunchSpec,
        opts: &clauseo_process::SpawnOptions,
    ) -> Spawned {
        Spawned {
            program: spec.program().to_path_buf(),
            args: spec.args().to_vec(),
            env: spec.env().clone(),
            cwd: spec.cwd().to_path_buf(),
            grace: opts.stdin_grace_set(),
            clears_env: opts.clears_env(),
        }
    }
}

/// A real process standing in for the outer: this test binary, running no test, which exits
/// at once.
pub(super) fn quick_outer() -> Result<clauseo_process::Launched, clauseo_process::Error> {
    let exe = std::env::current_exe().unwrap();
    let args = vec!["--exact".into(), "clauseo-no-such-test".into()];
    let quick = clauseo_process::LaunchSpec::new(exe, args, BTreeMap::new(), install()).unwrap();
    clauseo_process::spawn_in_tree(&quick, &clauseo_process::SpawnOptions::new())
}

impl Outer {
    /// A launch's denial pipe, as the world has it connect.
    pub(super) fn pipe(&self) -> FakePipe {
        FakePipe {
            adopts: *self.pipe_connects.lock().unwrap(),
            watched: Some(std::sync::Arc::clone(&self.watched)),
            ..FakePipe::default()
        }
    }

    /// The pids the launchers' pipes opened as the outer.
    pub(super) fn watched(&self) -> Vec<u32> {
        self.watched.lock().unwrap().clone()
    }

    /// The pool user's processes outlive `waits` waits for their exit (they tear down).
    pub(super) fn linger(&self, waits: u32) {
        *self.lingering.lock().unwrap() = waits;
    }

    /// `spawn_outer` starts a real process (this test binary, running no test), so the launch
    /// succeeds: the outer's two processes then run as the pool user until a wait for them.
    pub(super) fn spawn_starts(&self) {
        *self.spawn_starts.lock().unwrap() = true;
    }

    /// The denial pipe the last launch file named.
    pub(super) fn denial_pipe(&self) -> Option<String> {
        self.denial_pipe.lock().unwrap().clone()
    }

    /// What the last `spawn_outer` was given.
    pub(super) fn spawned(&self) -> Option<Spawned> {
        self.spawned.lock().unwrap().clone()
    }

    /// The pipe's one client is the outer, which connects and writes nothing.
    pub(super) fn pipe_connects(&self) {
        *self.pipe_connects.lock().unwrap() = true;
    }
}

/// The fake's denial pipe name.
pub(super) const PIPE: &str = r"\\.\pipe\clauseo-run-0123456789abcdef0123456789abcdef-denials";

/// A denial pipe: the outer it can open, the client that connects (if any), and what the
/// client writes, chunk by chunk; then the end, or a failed read. With `exits_after`, the
/// outer exits once that many reads have got bytes: the chunks left came after its exit. One
/// that `adopts` takes whatever pid it is given as the outer, which then connects.
#[derive(Default)]
pub(super) struct FakePipe {
    pub(super) outer: Option<Peer>,
    pub(super) client: Option<Peer>,
    pub(super) chunks: VecDeque<Vec<u8>>,
    pub(super) read_fails: bool,
    pub(super) exits_after: Option<usize>,
    pub(super) adopts: bool,
    /// Where it notes each pid it is asked to open as the outer.
    pub(super) watched: Option<std::sync::Arc<Mutex<Vec<u32>>>>,
}

impl DenialPipe for FakePipe {
    fn watch(&mut self, pid: u32) -> std::io::Result<Peer> {
        if let Some(watched) = &self.watched {
            watched.lock().unwrap().push(pid);
        }
        if self.adopts {
            let outer = Peer { pid, started: 1 };
            (self.outer, self.client) = (Some(outer), Some(outer));
        }
        self.outer
            .filter(|outer| outer.pid == pid)
            .ok_or_else(|| std::io::Error::other("no such process"))
    }

    fn accept(&mut self, _within: Duration) -> std::io::Result<Option<Peer>> {
        Ok(self.client)
    }

    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<Got> {
        if let Some(reads) = &mut self.exits_after {
            if *reads == 0 {
                return Ok(Got::Exited);
            }
            *reads -= 1;
        }
        let Some(mut chunk) = self.chunks.pop_front() else {
            return if self.read_fails {
                Err(std::io::Error::other("the pipe broke"))
            } else {
                Ok(Got::Closed)
            };
        };
        if chunk.len() > buf.len() {
            self.chunks.push_front(chunk.split_off(buf.len()));
        }
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok(Got::Bytes(chunk.len()))
    }
}
