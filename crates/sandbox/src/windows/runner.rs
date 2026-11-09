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
//! at the session's end, from the outer, once its job ended and emptied:
//!     --ITaskService (IRegisteredTask::Run, as the desktop user)-->
//!   \Clauseo\SandboxSweep   (the Task Scheduler's: SYSTEM, the pool users' tasks and BITS jobs)
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
//!   Startup entries, with no COM (`leftovers.rs`); nothing else removes them. The pool user's
//!   scheduled tasks and BITS jobs are the `SYSTEM` sweep task's.
//!   A pool user serves one live session at a time (`pool::assign::busy`), so none of those
//!   processes is another session's.
//! - At any end (the inner exiting, once the output drained; the host's stdin ending, once the
//!   agent, its stdin closed, had 1 s to finish; a failure, at once) the outer ends the job,
//!   waits up to 400 ms for it to empty, and then, once, asks the Task Scheduler to run the
//!   `SYSTEM` task `\Clauseo\SandboxSweep` (`end.rs`, `refresh::request_pool_sweep`), even when
//!   the job didn't empty: it deletes every pool user's scheduled tasks and cancels their BITS
//!   jobs, and kills nothing. The run is queued as the request returns, so the host ending the
//!   outer leaves it running. A request that fails is logged and changes no exit code; the outer,
//!   if it lives, waits up to 20 s for the run's result. No process of the pool user's is
//!   started at the end.
//! - The outer watches the matter's folder for a new `.git` while the agent runs, and sets one
//!   aside at once, ending the session first when it can't (`git_watch.rs`, `outer/watch.rs`).
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

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use super::denial_pipe::check_pipe_name;
use super::pool::pool_number;
use super::sids::Sid;

mod args;
#[cfg(windows)]
mod ca;
#[cfg(windows)]
mod desktop;
#[cfg(any(windows, test))]
pub(in crate::windows) mod end;
pub mod env;
pub mod frame;
#[cfg(any(windows, test))]
pub(crate) mod git_watch;
#[cfg(windows)]
mod inner;
#[cfg(any(windows, test))]
mod inner_flow;
#[cfg(any(windows, test))]
mod inner_user;
mod launch_debug;
#[cfg(any(windows, test))]
mod leftovers;
#[cfg(any(windows, test))]
mod log_file;
#[cfg(any(windows, test))]
pub mod marker;
#[cfg(windows)]
mod outer;
pub mod paths;
mod policy;
#[cfg(any(windows, test))]
mod previous;
mod run_exit;
#[cfg(windows)]
mod spawn;
#[cfg(any(windows, test))]
mod start;
#[cfg(any(windows, test))]
mod station;
#[cfg(any(windows, test))]
mod sweep;
#[cfg(any(windows, test))]
pub mod unclean;

pub use args::{Mode, mode, runner_args};
pub use env::agent_env;
#[cfg(windows)]
pub use inner::run_inner;
#[cfg(any(windows, test))]
pub use marker::{UncleanSession, unclean_sessions};
#[cfg(windows)]
pub use outer::run_outer;
use paths::is_batch;
pub use paths::{WinPath, codex_reuse};
#[cfg(windows)]
pub use policy::codex_link_in_policy;
pub use policy::{codex_reuse_in_policy, granted_paths};
pub use run_exit::{RunExit, agent_code};
/// The s15 harness's control stations read their names the way the outer does.
#[cfg(any(windows, test))]
pub use station::station_name_of_path;
#[cfg(any(windows, test))]
pub use unclean::StartUp;
#[cfg(windows)]
pub use unclean::os::start_up_requests;

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
    /// The matter's ID: the set-aside the outer's git watch moves a new `.git` into.
    pub matter: MatterId,
    /// The matter's folder, the run's one writable matter root, which the outer watches for a
    /// new `.git` (`git_watch.rs`); a granted path, never a read-only one.
    pub matter_root: String,
    /// The matter's notes folder, when the run can write one: watched as the matter's folder is.
    /// A granted path.
    #[serde(default)]
    pub notes_root: Option<String>,
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
    /// The host's denial pipe (`denial_pipe`), which the git watch's records go to; with none,
    /// or one that can't be opened, they are only logged, never said on stderr.
    #[serde(default)]
    pub denial_pipe: Option<String>,
}

impl LaunchMessage {
    /// The message for `spec` run as `pool` under `policy` for `matter`, with the CA bundle
    /// `ca_bundle`: the spec's program, arguments, folder and whole environment; the policy's
    /// matter folder, state folder, session temp folder, runtime paths, the paths it grants and
    /// its credential link. Refused when a path or variable isn't UTF-8 or breaks
    /// [`LaunchMessage::check`]'s rules.
    pub fn from_policy(
        policy: &crate::Policy,
        matter: MatterId,
        pool: &super::pool::PoolUser,
        spec: &clauseo_process::LaunchSpec,
        ca_bundle: Option<&Path>,
    ) -> Result<LaunchMessage, LaunchError> {
        policy::message_from_policy(policy, matter, pool, spec, ca_bundle)
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
    /// arguments and variables; a denial pipe's name as `denial_pipe::check_pipe_name` makes it.
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
        local("matter_root", &self.matter_root)?;
        if !self.granted.contains(&self.matter_root) {
            return Err(field("matter_root", "is not a granted path"));
        }
        if let Some(notes) = &self.notes_root {
            local("notes_root", notes)?;
            if !self.granted.contains(notes) {
                return Err(field("notes_root", "is not a granted path"));
            }
        }
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
        if let Some(pipe) = &self.denial_pipe {
            check_pipe_name(pipe)
                .map_err(|_bad| field("denial_pipe", "is not a denial pipe's name"))?;
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

/// The UI limits the launch's job carries: every `JOB_OBJECT_UILIMIT_*` flag (`HANDLES` `0x1`,
/// `READCLIPBOARD` `0x2`, `WRITECLIPBOARD` `0x4`, `SYSTEMPARAMETERS` `0x8`, `DISPLAYSETTINGS`
/// `0x10`, `GLOBALATOMS` `0x20`, `DESKTOP` `0x40`, `EXITWINDOWS` `0x80`), behind the agent's own
/// window station. `spawn::UI_LIMITS` is checked against it when it compiles.
pub const UI_LIMIT_MASK: u32 = 0xFF;

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

#[cfg(test)]
#[cfg(windows)]
mod job_tests;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;
