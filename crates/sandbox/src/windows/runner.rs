//! The tier-B runner (S15 `PR5`): how an agent starts as its matter's pool user, under the (B)
//! token, at Low, on a private desktop, with its stdio on the runner's own pipes.
//!
//! ```text
//! host --spawn_in_tree--> clauseo-sandbox-run --launch <data_dir>\sandbox\run\<16 hex>.json
//!   (the outer runner: the desktop user, in the host's job, its stdio the host's pipes)
//!     --CreateProcessWithLogonW(LOGON_WITH_PROFILE, suspended, in a kill-on-close job)-->
//!   clauseo-sandbox-run --inner <pipe base>   (the inner runner: the pool user, Medium)
//!     --CreateProcessAsUserW((B) token, private desktop, the in/out/err pipes)-->
//!   the agent (the pool user, restricted, Low)
//!
//! at the session's end, from the outer, outside every job of the outer's:
//!     --CreateProcessWithLogonW(LOGON_WITH_PROFILE, suspended, shut, resumed)-->
//!   clauseo-sandbox-run --sweep   (the end sweeper: the pool user, Medium, detached)
//! ```
//!
//! - The host writes a [`LaunchMessage`] ([`write_launch`]) into the protected run folder and
//!   starts [`RUNNER_EXE`] with [`runner_args`] through `spawn_in_tree`, so its `Launched`, job
//!   and shutdown ladder work as for any agent. The outer reads the file only if the desktop
//!   user owns it and only the desktop user, `SYSTEM` and `Administrators` may open it, then
//!   removes it.
//! - The outer reads the pool user's password from `secrets.bin` (DPAPI, `secrets::secret_for`),
//!   holds it in a buffer wiped on drop only until `CreateProcessWithLogonW` returns, and puts
//!   the suspended inner runner in a job with `KILL_ON_JOB_CLOSE` and no breakaway before it
//!   resumes it. The agent and everything it starts are in that job; the job's one handle is the
//!   outer's, so when the host's job ends the outer, the inner runner and the agent's tree end
//!   with it. A disabled Secondary Logon service (error 1058) is [`RunExit::SecondaryLogon`],
//!   `SetUpFailed` with its named cause ([`RunExit::no_sandbox`]).
//! - The pool user's processes outside the job are killed by the pool user itself (`sweep.rs`),
//!   which alone can see them all (`WTSEnumerateProcessesW` lists another user's processes only
//!   to Administrators): the inner runner's first act once resumed, before it reads the launch,
//!   then every [`TICK`](sweep::TICK) while the agent runs and once more when it exited
//!   (`inner_flow.rs`); the inner reads its own job's pid list for that, with no handle. The
//!   first act also removes the pool user's per-user COM classes, `Run`/`RunOnce` values and
//!   Startup entries, with no COM; the end sweeper removes those and then, over COM, its scheduled
//!   tasks (`leftovers.rs`; no BITS call: the `SYSTEM` stop task cancels a pool user's jobs).
//!   A pool user serves one live session at a time (`pool::assign::busy`), so none of those
//!   processes is another session's.
//! - At any end (the inner exiting, once the output drained; the host's stdin ending, once the
//!   agent, its stdin closed, had 1 s to finish; a failure, at once) the outer ends the job,
//!   waits up to 400 ms for it to empty, and then starts the end sweeper once (`end.rs`,
//!   `sweeper.rs`): `--sweep`, the same kills with no job kept and the same removal, on its
//!   own timeout. A job that doesn't empty starts no sweeper, nor does an end with less than
//!   500 ms of the host's 2 s after its stdin end left. The sweeper is never in the outer's
//!   job, so the host ending the outer leaves it running.
//! - The outer writes an unclean-session marker in the run folder once it has read the launch and
//!   removes it only at a clean end (`marker.rs`); `unclean_sessions` lists those left.
//! - The four pipes (`pipe.rs`) are the outer's, with the explicit DACL. The outer sends the
//!   launch on `ctl` only after it checked the client is the inner runner's pid and that pid's
//!   token user is the pool user; `CreateProcessWithLogonW` caps the command line at 1024
//!   characters, so nothing but the pipe base goes there.
//! - The inner runner sets `SetErrorMode`, makes the private desktop and the (B) token
//!   (`token.rs`), makes the profile folders the environment names, and starts the agent with
//!   only its three pipe handles inherited (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`). It exits with
//!   the agent's exit code, which the outer passes on; a runner failure exits with a
//!   [`RunExit`] code and one line on stderr.
//! - The agent's environment is [`env::agent_env`]: the profile variables under the state folder,
//!   git over OpenSSL, the CA bundle, and nothing that points into the lawyer's profile.
//! - **M3:** tier B never touches the lawyer's `~\.codex`: a launch whose credential link is that
//!   folder's `auth.json` (by file ID), or whose paths or `CODEX_HOME` lie in it, is refused
//!   ([`codex_reuse`], and the outer's file-ID check).
//!
//! The message, its checks, the environment and the exit codes hold no Windows call, so their
//! tests run on every OS.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::pool::pool_number;
#[cfg(any(windows, test, feature = "test-support"))]
use super::setup::rights_policy::RightPolicy;
use super::sids::Sid;

#[cfg(windows)]
mod ca;
#[cfg(windows)]
mod desktop;
#[cfg(any(windows, test))]
mod end;
pub mod env;
pub mod frame;
#[cfg(windows)]
mod inner;
#[cfg(any(windows, test))]
mod inner_flow;
#[cfg(any(windows, test))]
mod leftovers;
#[cfg(any(windows, test))]
pub mod marker;
#[cfg(windows)]
mod outer;
pub mod paths;
mod policy;
#[cfg(windows)]
mod spawn;
#[cfg(any(windows, test))]
mod start;
#[cfg(any(windows, test))]
mod sweep;
#[cfg(windows)]
mod sweeper;
#[cfg(any(windows, test))]
mod sweeper_flow;

pub use env::agent_env;
#[cfg(windows)]
pub use inner::run_inner;
#[cfg(any(windows, test))]
pub use marker::{UncleanSession, unclean_sessions};
#[cfg(windows)]
pub use outer::run_outer;
pub use paths::{WinPath, codex_reuse};
#[cfg(windows)]
pub use policy::codex_link_in_policy;
pub use policy::{codex_reuse_in_policy, granted_paths};
#[cfg(windows)]
pub use sweeper::run_sweep;

/// The runner's file name in the install folder. Kept stable: per-user installs grant it to the
/// pool users (S15 `PR8`).
pub const RUNNER_EXE: &str = "clauseo-sandbox-run.exe";
/// The run folder, in `<data_dir>\sandbox\`.
pub const RUN_DIR: &str = "run";
/// The launch message's format version.
pub const LAUNCH_VERSION: u32 = 1;
/// The largest launch file or frame read.
pub const LAUNCH_CAP: u64 = 1024 * 1024;
/// The most arguments and variables a launch may carry.
const ITEMS_CAP: usize = 4096;
/// Where the runner's own failures start among exit codes: the customer bit (`0x20000000`)
/// with the severity of an error, clear of any status Windows or an agent gives.
pub const RUN_EXIT_BASE: u32 = 0xE0C1_5000;

/// Why a launch message was refused. Its text names the field, never a value (the environment
/// holds the proxy's token).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    /// Not JSON of this shape, or too large.
    #[error("the launch message can't be read: {0}")]
    Format(String),
    /// Another format version.
    #[error("the launch message is version {0}, this runner reads {LAUNCH_VERSION}")]
    Version(u32),
    /// A field breaks a rule; which field and which rule.
    #[error("the launch message's {field} {rule}")]
    Field {
        /// The field.
        field: &'static str,
        /// The rule.
        rule: &'static str,
    },
}

/// What the host tells the outer runner: who to run as, what to run, and the folders the
/// environment names. Written by [`write_launch`], read by the outer runner alone.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchMessage {
    /// [`LAUNCH_VERSION`].
    pub version: u32,
    /// The matter's pool user.
    pub pool_user: Sid,
    /// Its account name, `ClauseoSbNN`; the outer checks the logged-on token's user is
    /// `pool_user`.
    pub pool_name: String,
    /// The matter's SID, added as a restricting SID when set (0049: off by default).
    #[serde(default)]
    pub matter_sid: Option<Sid>,
    /// The agent's program, absolute; never a batch file.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its working folder, absolute.
    pub cwd: String,
    /// The launch spec's whole environment; [`env::agent_env`] makes the agent's from it.
    pub env: Vec<(String, String)>,
    /// The agent's state folder.
    pub state: String,
    /// The session's temp folder (`Policy::session_tmpdir`), else `<state>\tmp`.
    #[serde(default)]
    pub tmpdir: Option<String>,
    /// The policy's runtime paths: `PATH` entries in the lawyer's profile stay only inside these.
    #[serde(default)]
    pub runtime: Vec<String>,
    /// Every path the policy grants (matter, state, notes, temp, runtime, read, allowed
    /// folders, Claude store, credential link): a variable whose value lies in the lawyer's
    /// profile stays only inside these.
    #[serde(default)]
    pub granted: Vec<String>,
    /// The CA bundle (`ca_bundle::ensure`), for `CODEX_CA_CERTIFICATE` and `SSL_CERT_FILE`.
    #[serde(default)]
    pub ca_bundle: Option<String>,
    /// The credential link in the state folder, for M3's file-ID check.
    #[serde(default)]
    pub credential_link: Option<String>,
    /// A firm's CA files, whose certificates the outer runner appends to the CA bundle
    /// (`ca_bundle::merge`). R17 PR-E fills it from `network.ca_certs`; the spec's own
    /// `SSL_CERT_FILE` is merged the same way.
    #[serde(default)]
    pub firm_ca: Vec<String>,
}

impl std::fmt::Debug for LaunchMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The environment's values (the proxy's token among them) are never printed.
        let names: Vec<&str> = self.env.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("LaunchMessage")
            .field("version", &self.version)
            .field("pool_user", &self.pool_user)
            .field("pool_name", &self.pool_name)
            .field("matter_sid", &self.matter_sid)
            .field("program", &self.program)
            .field("args", &self.args.len())
            .field("cwd", &self.cwd)
            .field("env", &names)
            .field("state", &self.state)
            .field("tmpdir", &self.tmpdir)
            .field("firm_ca", &self.firm_ca)
            .finish_non_exhaustive()
    }
}

impl LaunchMessage {
    /// The message for `spec` run as `pool` under `policy`, with the CA bundle `ca_bundle`:
    /// the spec's program, arguments, folder and whole environment; the policy's state folder,
    /// session temp folder, runtime paths, the paths it grants and its credential link. Refused
    /// when a path or variable isn't UTF-8 or breaks [`LaunchMessage::check`]'s rules.
    pub fn from_policy(
        policy: &crate::Policy,
        pool: &super::pool::PoolUser,
        spec: &clauseo_process::LaunchSpec,
        ca_bundle: Option<&Path>,
    ) -> Result<LaunchMessage, LaunchError> {
        policy::message_from_policy(policy, pool, spec, ca_bundle)
    }

    /// Reads and checks a launch message: [`LaunchMessage::check`]'s rules.
    pub fn parse(bytes: &[u8]) -> Result<LaunchMessage, LaunchError> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > LAUNCH_CAP {
            return Err(LaunchError::Format(format!(
                "larger than {LAUNCH_CAP} bytes"
            )));
        }
        let message: LaunchMessage = serde_json::from_slice(bytes).map_err(|error| {
            // serde's own text can quote a value (an unknown variant, a wrong type), and the
            // environment holds the proxy's token: only where it failed is said.
            LaunchError::Format(format!(
                "not a launch message (line {}, column {})",
                error.line(),
                error.column()
            ))
        })?;
        message.check()?;
        Ok(message)
    }

    /// The message as the file's bytes, checked first.
    pub fn to_bytes(&self) -> Result<Vec<u8>, LaunchError> {
        self.check()?;
        serde_json::to_vec(self).map_err(|error| LaunchError::Format(error.to_string()))
    }

    /// The rules: this version; a pool name `ClauseoSbNN`; the program, folders and paths full
    /// local paths with no `.` or `..` part and no NUL; the program no batch file; variable names
    /// non-empty with no NUL and no `=` but a leading one, values with no NUL; at most 4096
    /// arguments and variables.
    pub fn check(&self) -> Result<(), LaunchError> {
        if self.version != LAUNCH_VERSION {
            return Err(LaunchError::Version(self.version));
        }
        if pool_number(&self.pool_name).is_none() {
            return Err(field("pool_name", "is not a pool user's name"));
        }
        local("program", &self.program)?;
        if is_batch(&self.program) {
            return Err(field("program", "is a batch file"));
        }
        local("cwd", &self.cwd)?;
        local("state", &self.state)?;
        if WinPath::parse(&self.state).is_none_or(|state| state.drive().is_none()) {
            return Err(field("state", "is not on a drive letter"));
        }
        for (name, value) in [
            ("tmpdir", &self.tmpdir),
            ("ca_bundle", &self.ca_bundle),
            ("credential_link", &self.credential_link),
        ] {
            if let Some(value) = value {
                local(name, value)?;
            }
        }
        for path in &self.runtime {
            local("runtime", path)?;
        }
        for path in &self.granted {
            local("granted", path)?;
        }
        for path in &self.firm_ca {
            local("firm_ca", path)?;
        }
        if self.args.len() > ITEMS_CAP || self.env.len() > ITEMS_CAP {
            return Err(field("args or env", "has more than 4096 items"));
        }
        if self.args.iter().any(|arg| arg.contains('\0')) {
            return Err(field("args", "hold a NUL"));
        }
        for (name, value) in &self.env {
            // A leading `=` is cmd's per-drive folder (`=C:`), which `agent_env` drops.
            if name.is_empty() || name.contains('\0') || name.chars().skip(1).any(|c| c == '=') {
                return Err(field("env", "has a name that is empty or holds = or NUL"));
            }
            if value.contains('\0') {
                return Err(field("env", "has a value that holds a NUL"));
            }
        }
        Ok(())
    }
}

/// The launch file's path for `id` (16 lowercase hex): `<data_dir>\sandbox\run\<id>.json`.
pub fn launch_path(data_dir: &Path, id: &str) -> PathBuf {
    data_dir
        .join(super::matters::SANDBOX_DIR)
        .join(RUN_DIR)
        .join(format!("{id}.json"))
}

/// Whether `name` is a launch file's name: `<16 lowercase hex>.json`.
pub fn is_launch_name(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// What [`RUNNER_EXE`] was started to do, from its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `--launch <file>`: the outer runner.
    Launch(PathBuf),
    /// `--inner <pipe base>`: the inner runner.
    Inner(String),
    /// `--sweep`: the end sweeper, which takes nothing else.
    Sweep,
}

/// The mode `args` (the arguments after the program) name; `None` for any other command line.
pub fn mode(args: &[std::ffi::OsString]) -> Option<Mode> {
    match args {
        [flag, file] if flag == "--launch" => Some(Mode::Launch(PathBuf::from(file))),
        [flag, base] if flag == "--inner" => base.to_str().map(|base| Mode::Inner(base.to_owned())),
        [flag] if flag == "--sweep" => Some(Mode::Sweep),
        _ => None,
    }
}

/// The arguments the host starts [`RUNNER_EXE`] with.
pub fn runner_args(launch_file: &Path) -> Vec<std::ffi::OsString> {
    vec!["--launch".into(), launch_file.as_os_str().to_owned()]
}

/// Writes `message` to a new launch file in `<data_dir>\sandbox\run\` (made protected if
/// missing), atomically, and gives its path for [`runner_args`].
pub fn write_launch(data_dir: &Path, message: &LaunchMessage) -> std::io::Result<PathBuf> {
    let bytes = message
        .to_bytes()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let sandbox = data_dir.join(super::matters::SANDBOX_DIR);
    // The sandbox folder first, so it is never made by the run folder's `create_dir_all`
    // without its protected DACL.
    super::write::make_sandbox_dir(&sandbox)?;
    super::write::make_sandbox_dir(&sandbox.join(RUN_DIR))?;
    let mut bits = [0u8; 8];
    getrandom::fill(&mut bits).map_err(|error| std::io::Error::other(error.to_string()))?;
    let id = super::pipe::hex(&bits);
    let path = launch_path(data_dir, &id);
    super::write::write_atomic(&path, &bytes)?;
    // The file's own protected DACL (the desktop user, SYSTEM, Administrators), which the outer
    // runner requires before it reads it (`private_dir::read_unchanged`).
    super::write::check_owned_file(&path)?;
    #[cfg(windows)]
    super::private_dir::own_file(&path)?;
    Ok(path)
}

/// The runner's own failures, as its exit code (`RUN_EXIT_BASE + n`). Appended, never
/// renumbered: the host reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    /// The command line isn't `--launch <file>`, `--inner <base>` or `--sweep`.
    Usage,
    /// The launch file is missing, isn't the desktop user's alone, or breaks a rule.
    BadLaunch,
    /// `secrets.bin` has no password for the pool user: set-up must run again.
    NoSecret,
    /// The Secondary Logon service is disabled (1058).
    SecondaryLogon,
    /// "Allow log on locally" doesn't include the pool user (1385), and no policy is known to
    /// leave the pool out ([`logon_refusal`]): setting up again usually fixes it.
    LogonRight,
    /// The pool user's password was refused (1326): set-up must run again.
    Password,
    /// Another logon failure, or what the outer needs before its logon failed to be read: the
    /// pool user's secret, System32's path. A marker that can't be written is
    /// [`RunExit::MarkerUnwritable`].
    Logon,
    /// M3: the launch would reuse the lawyer's `~\.codex`.
    CodexReuse,
    /// A runner pipe couldn't be made, connected or checked.
    Pipe,
    /// The inner runner couldn't be put in the job, or the job made.
    Job,
    /// The inner runner's token, desktop or folders couldn't be made.
    Token,
    /// The agent couldn't be started.
    Spawn,
    /// The agent exited with a code in the runner's own range, which the outer runner never
    /// passes on as it is ([`RUN_EXIT_BASE`] to `+0xff`).
    AgentReserved,
    /// The inner runner's first-act sweep of the pool user's processes failed (`sweep.rs`): the
    /// process list couldn't be read, or a process of the pool user's couldn't be checked, or it
    /// kept starting processes. Also the end sweeper's exit code when anything it did failed but
    /// for an escapee. A process that couldn't be opened or ended is [`RunExit::Escapee`].
    Sweep,
    /// The runner's DLL search couldn't be kept to System32.
    DllSearch,
    /// A sweep found a process of the pool user's it couldn't end, even after the owner's
    /// `WRITE_DAC` reopen: one that shut its DACL to its own user, alive outside every job (M2).
    /// The inner's first act exits with it and starts no agent; the end sweeper exits with it.
    /// The outer exits with it whenever any sweep of its session found one: the first act's, the
    /// inner's ticks and end (reported on `ctl`), or the end sweeper's (its exit code, when the
    /// outer still waits for it). It wins over the agent's own code, even 0, which the outer then
    /// says on stderr: the run's output came through, but the pool user has a process no sweep of
    /// its own can end, and only the host can act on that (0049).
    Escapee,
    /// Reserved, no longer produced. The end sweeper once exited with it when everything went
    /// but BITS jobs of the pool user's it couldn't cancel (1245). It makes no BITS call now
    /// (`leftovers.rs`), and the outer takes this code as any failed sweeper's. Kept, with its
    /// number, since the codes are explicit and may be read downstream.
    PoolStop,
    /// The session's unclean-session marker couldn't be written, or the launch file's name gives
    /// it no id (`marker::marked`): the outer refuses the launch before any logon, so no session
    /// runs that no marker names.
    MarkerUnwritable,
    /// A 1385 where a Group Policy or Intune setting defines "Allow log on locally" without the
    /// pool, as the cached policy read says ([`logon_refusal`]): setting up again doesn't fix it.
    LogonPolicy,
}

impl RunExit {
    const ALL: [RunExit; 19] = [
        RunExit::Usage,
        RunExit::BadLaunch,
        RunExit::NoSecret,
        RunExit::SecondaryLogon,
        RunExit::LogonRight,
        RunExit::Password,
        RunExit::Logon,
        RunExit::CodexReuse,
        RunExit::Pipe,
        RunExit::Job,
        RunExit::Token,
        RunExit::Spawn,
        RunExit::AgentReserved,
        RunExit::Sweep,
        RunExit::DllSearch,
        RunExit::Escapee,
        RunExit::PoolStop,
        RunExit::MarkerUnwritable,
        RunExit::LogonPolicy,
    ];

    /// The exit code.
    pub fn code(self) -> u32 {
        let index = RunExit::ALL
            .iter()
            .position(|exit| *exit == self)
            .unwrap_or(0);
        RUN_EXIT_BASE + u32::try_from(index).unwrap_or(0)
    }

    /// The failure an exit code names, if it is one.
    pub fn from_code(code: u32) -> Option<RunExit> {
        let index = usize::try_from(code.checked_sub(RUN_EXIT_BASE)?).ok()?;
        RunExit::ALL.get(index).copied()
    }

    /// What tier detection makes of it: the set-up's failures, which running the set-up again or
    /// IT fixes, as `SetUpFailed` or `SetUpBlocked` with a named cause; `None` for the rest,
    /// which fail this launch only.
    pub fn no_sandbox(self) -> Option<crate::NoSandbox> {
        use crate::NoSandbox;
        match self {
            RunExit::SecondaryLogon => Some(NoSandbox::SetUpFailed(SECLOGON_DISABLED.to_owned())),
            RunExit::LogonRight => Some(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight)),
            RunExit::LogonPolicy => Some(NoSandbox::SetUpBlocked(crate::Blocked::LogonPolicy)),
            RunExit::NoSecret | RunExit::Password => {
                Some(NoSandbox::SetUpFailed(PASSWORD_LOST.to_owned()))
            }
            RunExit::Usage
            | RunExit::BadLaunch
            | RunExit::Logon
            | RunExit::CodexReuse
            | RunExit::Pipe
            | RunExit::Job
            | RunExit::Token
            | RunExit::Spawn
            | RunExit::AgentReserved
            | RunExit::Sweep
            | RunExit::DllSearch
            | RunExit::Escapee
            | RunExit::PoolStop
            | RunExit::MarkerUnwritable => None,
        }
    }
}

/// What a launch's 1385 is, from what the cached policy read says of "Allow log on locally"
/// (`setup::rights_source::cached`): a policy that defines it without the pool is
/// [`RunExit::LogonPolicy`], with [`Blocked::LogonPolicy`](crate::Blocked::LogonPolicy)'s words;
/// anything else, an unreadable policy included, stays [`RunExit::LogonRight`].
#[cfg(any(windows, test, feature = "test-support"))]
pub fn logon_refusal(policy: RightPolicy) -> (RunExit, crate::Blocked) {
    match policy {
        RightPolicy::Defined {
            includes_pool: false,
        } => (RunExit::LogonPolicy, crate::Blocked::LogonPolicy),
        RightPolicy::Defined {
            includes_pool: true,
        }
        | RightPolicy::NotDefined
        | RightPolicy::Unknown => (RunExit::LogonRight, crate::Blocked::LogonRight),
    }
}

/// The agent's exit code as the runner passes it on: one in the runner's own range
/// (`RUN_EXIT_BASE` to `+0xff`) becomes [`RunExit::AgentReserved`], so an agent can never pass
/// for a runner failure (a set-up one among them) in the host's eyes.
pub fn agent_code(code: u32) -> u32 {
    if (RUN_EXIT_BASE..=RUN_EXIT_BASE + 0xff).contains(&code) {
        RunExit::AgentReserved.code()
    } else {
        code
    }
}

/// The agent's window station's name, `ClauseoRun-<16 hex>`, from 64 bits of its own randomness:
/// nothing another process can read first (the pipe base is in the `\\.\pipe\` listing), and
/// the station is made only if no station has the name (`CWF_CREATE_ONLY`).
pub fn station_name(random: [u8; 8]) -> String {
    format!("ClauseoRun-{}", super::pipe::hex(&random))
}

/// The cause when the Secondary Logon service is disabled: set-up's own words.
pub const SECLOGON_DISABLED: &str = "the Secondary Logon service (seclogon) is disabled: IT can \
     set it to start manually";
/// The cause when the pool user's password is missing or refused.
pub const PASSWORD_LOST: &str = "the sandbox user's saved password is missing or out of date: \
     run the set-up again";

fn field(field: &'static str, rule: &'static str) -> LaunchError {
    LaunchError::Field { field, rule }
}

/// Refuses `value` unless it is a full local path with no `.` or `..` part.
fn local(name: &'static str, value: &str) -> Result<(), LaunchError> {
    match WinPath::parse(value) {
        Some(_) => Ok(()),
        None => Err(field(
            name,
            "is not a full local path without . or .. parts",
        )),
    }
}

/// Whether `program` names a batch file: `.cmd` or `.bat`, in any case, trailing dots and
/// spaces dropped as Windows drops them.
fn is_batch(program: &str) -> bool {
    let trimmed = program.trim_end_matches(['.', ' ']);
    let extension = trimmed.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

#[cfg(test)]
#[cfg(windows)]
mod job_tests;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;
