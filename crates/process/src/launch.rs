//! Starting an agent: the [`Launcher`] seam, the passthrough launcher, and [`spawn_in_tree`], the
//! one place an agent process starts.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use async_process::ChildStdout;

use crate::handle::Handle;
use crate::stderr::{Redactor, Ring, StderrPump};
use crate::stdin::{AgentStdin, StdinSlot};
use crate::{Error, LaunchSpec, os};

/// Starts agents. `host` holds an `Arc<dyn Launcher>`; a sandbox launcher fixes its policy in
/// its constructor, so a launch can never widen it.
pub trait Launcher: Send + Sync {
    /// Starts the agent `spec` names.
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error>;
}

/// Starts the spec as it is, with no sandbox.
#[derive(Debug, Clone)]
pub struct PassthroughLauncher {
    opts: SpawnOptions,
}

impl PassthroughLauncher {
    /// A launcher that starts every spec with `opts`.
    pub fn new(opts: SpawnOptions) -> PassthroughLauncher {
        PassthroughLauncher { opts }
    }
}

impl Launcher for PassthroughLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        spawn_in_tree(spec, &self.opts)
    }
}

/// How [`spawn_in_tree`] starts an agent, beyond its spec.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SpawnOptions {
    pub(crate) home: Option<PathBuf>,
    #[cfg(windows)]
    pub(crate) creation_flags: u32,
}

impl SpawnOptions {
    /// No home folder, and no extra creation flags.
    pub fn new() -> SpawnOptions {
        SpawnOptions::default()
    }

    /// The home folder, which the stderr tail shows as `~`. The crate reads no variable to find
    /// it.
    pub fn home(mut self, home: PathBuf) -> SpawnOptions {
        self.home = Some(home);
        self
    }

    /// Process creation flags added to the crate's own (`CREATE_SUSPENDED`, `CREATE_NO_WINDOW`).
    #[cfg(windows)]
    pub fn creation_flags(mut self, flags: u32) -> SpawnOptions {
        self.creation_flags = flags;
        self
    }
}

/// A started agent. Write ACP to `stdin`, read it from `stdout`, drive `stderr`, and keep `handle`
/// for its exit and its shutdown. Dropping the handle kills the agent's tree.
#[derive(Debug)]
#[non_exhaustive]
pub struct Launched {
    /// The agent's stdin; the handle's shutdown closes it.
    pub stdin: AgentStdin,
    /// The agent's stdout.
    pub stdout: ChildStdout,
    /// The pump the caller must drive, or the agent stalls once its stderr pipe fills.
    pub stderr: StderrPump,
    /// The agent's process and its tree.
    pub handle: Handle,
}

/// Starts `spec` with stdio pipes in a tree of its own: a new process group on Unix, a Job Object
/// on Windows that it joins before it runs. The child inherits this process's environment, then
/// `spec`'s variables override it. No shell, ever.
pub fn spawn_in_tree(spec: &LaunchSpec, opts: &SpawnOptions) -> Result<Launched, Error> {
    let mut command = std::process::Command::new(spec.program());
    command
        .args(spec.args())
        .envs(spec.env())
        .current_dir(spec.cwd());
    os::prepare(&mut command, opts);
    let mut command = async_process::Command::from(command);
    // Set here, not on the std command: async-process's `spawn` resets to inherit every stream
    // its own setters did not set.
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|source| Error::Spawn {
        program: spec.program().to_owned(),
        source,
    })?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        // Dropping the child kills it: kill_on_drop.
        return Err(Error::Os {
            what: "the agent started without its pipes",
            source: std::io::Error::other("a piped stdio stream is missing"),
        });
    };
    let tree = os::Tree::adopt(&mut child)?;
    let ring = Arc::new(Mutex::new(Ring::new()));
    let redactor = Arc::new(Redactor::new(opts.home.as_deref(), spec.env()));
    let stdin = StdinSlot::new(stdin);
    Ok(Launched {
        stdin: AgentStdin::new(stdin.clone()),
        stdout,
        stderr: StderrPump::new(stderr, Arc::clone(&ring)),
        handle: Handle::new(child, tree, stdin, ring, redactor),
    })
}
