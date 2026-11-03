//! One agent's install job, on its own thread: up to three tries, each one checking the free
//! space, choosing Node, fetching every file (`files`), then npm's `cache add` and `ci` and the
//! rename and receipt (`npm`).
//!
//! - `Offline`: `Waiting`, then 30 s on the timer and again, with no try counted, until it
//!   works or is paused.
//! - `Blocked` and `DiskFull` stop at once: a retry would meet the same proxy or disk.
//! - Anything else counts as a try; the third is `Failed`, after waits of 2 s and then 8 s.
//! - A pause ends the job quietly where it is: `pause` has already sent `Paused`.
//!
//! Every wait and every npm run is raced against the job's [`Cancel`], and every event goes
//! through [`Shared::emit_job`], which drops it once the job is paused.

mod files;
mod npm;

use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;
use futures::future::{Either, select};

use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::disk;
use crate::download::{FetchError, FetchKind, write_kind};
use crate::log::{InstallLog, redact};
use crate::node::{self, FoundNode, NotUsed, UserSearch};
use crate::platform::Libc;
use crate::receipt::{install_folder, read_receipt, script_path, write_receipt};
use crate::state::{Failure, FailureKind, InstallState, Installed};
use crate::worker::{Shared, Ticket};
use crate::write::ExtractError;

/// The most tries one install makes.
const TRIES: u32 = 3;
/// The waits after the first and the second failed try.
const BACKOFF: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(8)];
/// The wait between tries while offline.
const OFFLINE_WAIT: Duration = Duration::from_secs(30);
/// Bytes in the log's and the failure's MB.
const MB: u64 = 1_000_000;

/// Everything one job needs, moved onto its thread.
pub(crate) struct Job<T> {
    pub(crate) ticket: Ticket,
    pub(crate) config: AgentsConfig,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) timer: Arc<T>,
    pub(crate) shared: Arc<Shared>,
    /// The job lock: held for the whole job, so jobs run one after another.
    pub(crate) turn: Arc<Mutex<()>>,
    /// The OS's proxy, when a retry asked for it.
    pub(crate) system_proxy: Option<String>,
    /// The lock on `agents/.lock` while the job works: `None` until taken, and given up before
    /// the job's last event, so the next job (here or in another process) never meets it.
    pub(crate) data_lock: Mutex<Option<File>>,
}

/// The job thread's body: waits for the job lock, then works unless paused meanwhile.
pub(crate) fn run<T: Timer + Send + Sync>(job: &Job<T>) {
    let _turn = job.turn.lock().unwrap_or_else(PoisonError::into_inner);
    if job.ticket.cancel.is_cancelled() {
        return;
    }
    futures::executor::block_on(job.work());
    // Given up before the job lock, so the next job here never finds it held.
    job.release_lock();
}

/// How a try ended badly, and what the job does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Wait 30 s and try again, no try counted.
    Offline,
    /// Stop now as `Blocked`.
    Blocked,
    /// Stop now as `DiskFull`.
    DiskFull,
    /// Count a try; the third is `Failed`.
    Retry,
    /// Stop now as `Failed`: a retry cannot help.
    Stop,
    /// Paused: end quietly.
    Cancelled,
}

/// A try that did not finish: what comes next, and the log's lines for it.
#[derive(Debug)]
pub(crate) struct TryError {
    next: Next,
    host: Option<String>,
    status: Option<u16>,
    /// Redacted.
    detail: String,
}

impl TryError {
    /// A try to count, with `detail` redacted.
    pub(crate) fn retry(detail: &str, config: &AgentsConfig) -> TryError {
        TryError::new(Next::Retry, detail, config)
    }

    fn new(next: Next, detail: &str, config: &AgentsConfig) -> TryError {
        TryError {
            next,
            host: None,
            status: None,
            detail: redact(detail, &config.data_dir),
        }
    }

    pub(crate) fn cancelled() -> TryError {
        TryError {
            next: Next::Cancelled,
            host: None,
            status: None,
            detail: String::new(),
        }
    }

    /// A local file error: a full disk, else a try to count.
    pub(crate) fn io(what: &str, err: &io::Error, config: &AgentsConfig) -> TryError {
        let next = match write_kind(err) {
            FetchKind::DiskFull => Next::DiskFull,
            _ => Next::Retry,
        };
        TryError::new(next, &format!("{what}: {err}"), config)
    }

    pub(crate) fn extract(err: &ExtractError, config: &AgentsConfig) -> TryError {
        match err {
            ExtractError::Io(err) => TryError::io("unpacking Node failed", err, config),
            other => TryError::retry(&other.to_string(), config),
        }
    }

    /// npm's `ENOSPC` is a full disk; any other npm failure is a try to count.
    pub(crate) fn npm(detail: &str, enospc: bool, config: &AgentsConfig) -> TryError {
        let next = if enospc { Next::DiskFull } else { Next::Retry };
        TryError::new(next, detail, config)
    }
}

impl From<FetchError> for TryError {
    fn from(err: FetchError) -> TryError {
        let next = match err.kind {
            FetchKind::Offline => Next::Offline,
            FetchKind::Blocked => Next::Blocked,
            FetchKind::DiskFull => Next::DiskFull,
            FetchKind::Cancelled => Next::Cancelled,
            FetchKind::Integrity | FetchKind::Failed => Next::Retry,
        };
        TryError {
            next,
            host: err.host,
            status: err.status,
            detail: err.detail,
        }
    }
}

/// The Node a try uses.
#[derive(Debug, Clone)]
pub(crate) enum NodeChoice {
    /// The user's, v22 or later.
    User(FoundNode),
    /// The downloaded runtime at `dir`; `fetch` when it is not unpacked there yet.
    Runtime { dir: PathBuf, fetch: bool },
}

/// What the job carries from try to try.
#[derive(Debug)]
pub(crate) struct Run {
    log: InstallLog,
    /// Tries counted so far.
    tries: u32,
    /// Tries made so far, offline ones included: the temp folder's number.
    pub(crate) attempts: u32,
    /// The last progress, for the log's `received:` line.
    pub(crate) received: u64,
    pub(crate) total: u64,
}

impl Run {
    fn new(agent: &AgentId) -> Run {
        Run {
            log: InstallLog::new(agent),
            tries: 0,
            attempts: 0,
            received: 0,
            total: 0,
        }
    }
}

impl<T> Job<T> {
    /// Checks the data folder and the bundle are absolute, then takes `agents/.lock`. Another
    /// process holding it (another Clauseo installing) fails the job at once: the single-instance
    /// plugin makes that rare, and a retry once it is done works.
    fn take_lock(&self) -> Result<(), TryError> {
        let config = &self.config;
        check_paths(config)?;
        match crate::write::try_lock_agents(&config.data_dir) {
            Ok(Some(file)) => {
                *self
                    .data_lock
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(file);
                Ok(())
            }
            Ok(None) => Err(TryError::new(
                Next::Stop,
                "another Clauseo is installing an agent now; try again once it is done",
                config,
            )),
            Err(err) => Err(TryError::io("locking the agents folder", &err, config)),
        }
    }

    /// Gives up `agents/.lock`, if held.
    fn release_lock(&self) {
        drop(
            self.data_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
}

/// The data folder, and the bundle when set, must be absolute paths: a relative one would
/// resolve against whatever the current folder is.
pub(crate) fn check_paths(config: &AgentsConfig) -> Result<(), TryError> {
    if !config.data_dir.is_absolute() {
        return Err(TryError::new(
            Next::Stop,
            "the data folder is not an absolute path",
            config,
        ));
    }
    if config
        .install
        .bundle
        .as_ref()
        .is_some_and(|bundle| !bundle.is_absolute())
    {
        return Err(TryError::new(
            Next::Stop,
            "the bundle setting is not an absolute path",
            config,
        ));
    }
    Ok(())
}

impl<T: Timer + Send + Sync> Job<T> {
    async fn work(&self) {
        let agent = &self.ticket.agent;
        let config = &self.config;
        if let Err(err) = self.take_lock() {
            return self.fail(FailureKind::Failed, &err, &Run::new(agent), None);
        }
        let search = {
            let find = pin!(node::search_user_node(
                &config.node_search,
                &*self.launcher,
                &*self.timer
            ));
            match select(find, pin!(self.ticket.cancel.cancelled())).await {
                Either::Left((search, _)) => search,
                Either::Right(((), _)) => return,
            }
        };
        let (launcher, timer) = (&*self.launcher, &*self.timer);
        if let Some(installed) = ready_install(config, launcher, timer, agent, &search).await {
            self.ready(installed);
            return;
        }
        let user = search.found;
        let mut run = Run::new(agent);
        loop {
            if self.ticket.cancel.is_cancelled() {
                return;
            }
            run.attempts += 1;
            let err = match self.attempt(user.as_ref(), &mut run).await {
                Ok(installed) => {
                    self.ready(installed);
                    return;
                }
                Err(err) => err,
            };
            match err.next {
                Next::Cancelled => return,
                Next::Offline => {
                    let mut log = run.log.clone();
                    push_error(&mut log, &err);
                    finish_log(&mut log, &run, FailureKind::Offline);
                    let state = InstallState::Waiting {
                        reason: FailureKind::Offline,
                        log,
                    };
                    if !self.emit(state, false) || !self.wait(OFFLINE_WAIT).await {
                        return;
                    }
                }
                Next::Blocked | Next::DiskFull | Next::Stop => {
                    run.tries += 1;
                    let kind = match err.next {
                        Next::Blocked => FailureKind::Blocked,
                        Next::DiskFull => FailureKind::DiskFull,
                        _ => FailureKind::Failed,
                    };
                    return self.fail(kind, &err, &run, user.as_ref());
                }
                Next::Retry => {
                    run.tries += 1;
                    let backoff = usize::try_from(run.tries - 1)
                        .ok()
                        .and_then(|n| BACKOFF.get(n).copied())
                        .filter(|_| run.tries < TRIES);
                    let Some(backoff) = backoff else {
                        return self.fail(FailureKind::Failed, &err, &run, user.as_ref());
                    };
                    push_error(&mut run.log, &err);
                    if !self.wait(backoff).await {
                        return;
                    }
                }
            }
        }
    }

    /// One try: Node, the space check, the files, then npm.
    async fn attempt(
        &self,
        user: Option<&FoundNode>,
        run: &mut Run,
    ) -> Result<Installed, TryError> {
        let config = &self.config;
        let platform = config.platform;
        let node = match user {
            Some(found) => NodeChoice::User(found.clone()),
            None if platform.libc == Some(Libc::Musl) => {
                return Err(TryError::new(
                    Next::Stop,
                    "the Node Clauseo downloads is a glibc build, which does not run on this \
                     musl Linux: install Node 22 or later",
                    config,
                ));
            }
            None => {
                let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), platform);
                NodeChoice::Runtime {
                    fetch: !runtime_check(config, &*self.launcher, &*self.timer)
                        .await
                        .usable(),
                    dir,
                }
            }
        };
        let node_needed = matches!(node, NodeChoice::Runtime { fetch: true, .. });
        self.check_disk(node_needed)?;
        let fetched = files::fetch_all(self, &node, run)?;
        let installed = npm::install(self, &node, &fetched, run).await?;
        files::clean_up(config, &fetched);
        Ok(installed)
    }

    /// `DiskFull` when the free space at the data folder is below the need. A free space that
    /// cannot be read is not checked.
    fn check_disk(&self, node_needed: bool) -> Result<(), TryError> {
        let (need, free) = self.space(node_needed);
        match free {
            Some(free) if free < need => Err(TryError::new(
                Next::DiskFull,
                &format!("{} MB free, {} MB needed", free / MB, need.div_ceil(MB)),
                &self.config,
            )),
            _ => Ok(()),
        }
    }

    /// The bytes this install needs, and the bytes free at the data folder.
    fn space(&self, node_needed: bool) -> (u64, Option<u64>) {
        let config = &self.config;
        let need = config
            .snapshot
            .need_bytes(&self.ticket.agent, config.platform, node_needed)
            .unwrap_or(0);
        (need, disk::free_bytes(&config.data_dir).ok())
    }

    /// Sends `state` unless the job is paused; `false` when it is.
    pub(crate) fn emit(&self, state: InstallState, ends: bool) -> bool {
        self.shared.emit_job(&self.ticket, state, None, ends)
    }

    fn ready(&self, installed: Installed) {
        self.release_lock();
        let version = installed.version.clone();
        self.shared.emit_job(
            &self.ticket,
            InstallState::Ready { version },
            Some(installed),
            true,
        );
    }

    /// Sends the job's last state, `Failed` of `kind`, with its log.
    fn fail(&self, kind: FailureKind, err: &TryError, run: &Run, user: Option<&FoundNode>) {
        self.release_lock();
        let mut log = run.log.clone();
        push_error(&mut log, err);
        finish_log(&mut log, run, kind);
        let (need_mb, free_mb) = if kind == FailureKind::DiskFull {
            let node_needed = user.is_none() && !runtime_ready(&self.config);
            let (need, free) = self.space(node_needed);
            (Some(need.div_ceil(MB)), free.map(|free| free / MB))
        } else {
            (None, None)
        };
        let failure = Failure {
            kind,
            need_mb,
            free_mb,
            log,
        };
        self.emit(InstallState::Failed(failure), true);
    }

    /// Waits `duration` on the timer; `false` when a pause came first.
    async fn wait(&self, duration: Duration) -> bool {
        let after = pin!(self.timer.after(duration));
        matches!(
            select(after, pin!(self.ticket.cancel.cancelled())).await,
            Either::Left(_)
        )
    }

    pub(crate) fn cancel(&self) -> &Cancel {
        &self.ticket.cancel
    }
}

/// Adds a failed try's host, status and detail to `log`.
fn push_error(log: &mut InstallLog, err: &TryError) {
    if let Some(host) = &err.host {
        log.push(format!("host: {host}"));
    }
    if let Some(status) = err.status {
        log.push(format!("status: {status}"));
    }
    if !err.detail.is_empty() {
        log.push(err.detail.clone());
    }
}

/// Adds the `received:` and `result:` lines.
fn finish_log(log: &mut InstallLog, run: &Run, kind: FailureKind) {
    log.push(format!(
        "received: {} of {} MB",
        run.received / MB,
        run.total.div_ceil(MB)
    ));
    log.push(format!("result: {} after {} tries", kind.word(), run.tries));
}

/// Whether the downloaded Node is unpacked and complete.
pub(crate) fn runtime_ready(config: &AgentsConfig) -> bool {
    let Some(archive) = config.snapshot.node_archive(config.platform) else {
        return false;
    };
    let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    node::runtime_valid(&dir, archive, config.platform.os)
}

/// What the start's check of the downloaded Node found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCheck {
    /// Not unpacked, or its receipt is missing or for another archive.
    Absent,
    /// It answers `--version` with v22 or later.
    Good,
    /// `--version` took too long or would not start: not proof of a bad Node, so it is kept and
    /// checked again at the next install or start.
    Unsure,
    /// A clear bad answer (a non-zero exit, no version, or one below 22): fetched again.
    Bad,
}

impl RuntimeCheck {
    /// Whether the runtime is used as it is.
    pub(crate) fn usable(self) -> bool {
        matches!(self, RuntimeCheck::Good | RuntimeCheck::Unsure)
    }
}

/// Checks the downloaded Node: unpacked and complete, then `--version` through the launcher, so
/// a runtime cut short by a power cut is fetched again.
pub(crate) async fn runtime_check<T: Timer>(
    config: &AgentsConfig,
    launcher: &dyn Launcher,
    timer: &T,
) -> RuntimeCheck {
    if !runtime_ready(config) {
        return RuntimeCheck::Absent;
    }
    let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    let program = node::runtime_program(&dir, config.platform.os);
    match node::check_node(&program, launcher, timer).await {
        Ok(_) => RuntimeCheck::Good,
        Err(NotUsed::TimedOut | NotUsed::LaunchFailed(_)) => RuntimeCheck::Unsure,
        Err(_) => RuntimeCheck::Bad,
    }
}

/// `agent`'s finished install: a valid receipt in its `<hash>` folder, run with a Node this
/// start would choose anyway. The receipt's Node is kept when it is the downloaded runtime and
/// usable ([`RuntimeCheck::usable`]), or the Node the `PATH` search finds now, or one that
/// search could not finish checking. Otherwise the receipt is rewritten (atomically) to name the
/// Node found now, else the usable runtime, and stays ready: the adapter runs on any v22 or
/// later. With neither, `None`, and the install is redone. A receipt never makes a program run
/// that this start would not choose (the data folder is trusted at rest, but a receipt marks
/// completeness, not which program may run).
pub(crate) async fn ready_install<T: Timer>(
    config: &AgentsConfig,
    launcher: &dyn Launcher,
    timer: &T,
    agent: &AgentId,
    user: &UserSearch,
) -> Option<Installed> {
    let entry = config.snapshot.agent(agent)?;
    let hash = config.snapshot.install_hash(agent, config.platform)?;
    let folder = install_folder(&config.data_dir, agent, &hash);
    let mut receipt = read_receipt(&folder, agent, &hash, config.platform)?;
    let runtime = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    let runtime = node::runtime_program(&runtime, config.platform.os);
    let mut runtime_usable = None;
    let keep = if receipt.node == runtime {
        let usable = runtime_check(config, launcher, timer).await.usable();
        runtime_usable = Some(usable);
        usable
    } else {
        user.found
            .as_ref()
            .is_some_and(|found| found.program == receipt.node)
            || user.timed_out.contains(&receipt.node)
    };
    if !keep {
        let (program, version) = if let Some(found) = &user.found {
            (found.program.clone(), found.version.clone())
        } else {
            let usable = match runtime_usable {
                Some(usable) => usable,
                None => runtime_check(config, launcher, timer).await.usable(),
            };
            if !usable {
                return None;
            }
            (runtime, format!("v{}", config.snapshot.node().version()))
        };
        receipt.node = program;
        receipt.node_version = version;
        // A receipt that will not be rewritten now is rewritten at the next start.
        drop(write_receipt(&folder, &receipt));
    }
    Some(Installed {
        node: receipt.node.clone(),
        script: script_path(&folder, entry),
        folder,
        version: receipt.version,
    })
}
