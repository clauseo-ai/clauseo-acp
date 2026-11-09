//! One agent's install job, on its own thread: up to three tries, each one checking the free
//! space, choosing Node, fetching every file (`files`), then `npm ci` reading the checked
//! tarballs through `file:` URLs, and the rename and receipt (`npm`).
//!
//! - `Offline`: `Waiting`, then 30 s on the timer and again, with no try counted, until it
//!   works or is paused.
//! - `Blocked` and `DiskFull` stop at once: a retry would meet the same proxy or disk.
//! - Anything else counts as a try; the third is `Failed`, after waits of 2 s and then 8 s. A
//!   sharing or lock error in `npm ci` or the rename is first retried in place (`npm`).
//!
//! Each try starts by emptying the npm cache ([`empty_npm_cache`]), so tries cut short never
//! pile up its copies of the tarballs; except a try whose set-up is kept from one that failed
//! only its rename, which renames it alone (`kept`). Every way to `Ready` then removes the
//! install's downloads and the npm cache ([`files::clean_up`]); a failed try keeps both.
//! - A pause ends the job quietly where it is: `pause` has already sent `Paused`.
//!
//! Every wait and every npm run is raced against the job's [`Cancel`], and every event goes
//! through [`Shared::emit_job`], which drops it once the job is paused.

mod files;
mod kept;
mod npm;
mod python;
mod ready;
mod try_error;

pub(crate) use files::empty_npm_cache;
#[cfg(test)]
pub(crate) use kept::MARKER;
pub(crate) use kept::marked_for;
use try_error::Next;
pub(crate) use try_error::TryError;

use std::fs::File;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;
use futures::future::{Either, select};

use crate::cancel::Cancel;
use crate::config::{AgentsConfig, Network};
use crate::disk;
use crate::log::InstallLog;
use crate::node::{self, FoundNode};
use crate::platform::Libc;
use crate::state::{Failure, FailureKind, InstallState, Installed};
use crate::worker::{Shared, Ticket};

pub(crate) use ready::{ready_install, runtime_check, runtime_ready};

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
    pub(crate) network: Arc<RwLock<Network>>, // `Agents::set_network`'s, read by each try
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
/// resolve against whatever the current folder is. On Windows the data folder must not be on a
/// share, which npm cannot install from ([`files::data_dir_on_share`]).
pub(crate) fn check_paths(config: &AgentsConfig) -> Result<(), TryError> {
    if files::data_dir_on_share(&config.data_dir.to_string_lossy(), config.platform.os) {
        return Err(TryError::stop(
            "the data folder is on a network share; npm cannot install from there",
            config,
        ));
    }
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
            // Finished already (a try cut short after its receipt, say): what it fetched goes.
            clean_up_finished(config, agent);
            self.ready(installed, 0).await;
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
                    self.ready(installed, run.tries + 1).await;
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
                        _ => err.failed_kind(),
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
                        return self.fail(err.failed_kind(), &err, &run, user.as_ref());
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
        // A finished set-up whose rename failed: only the rename runs again.
        // The earlier try's downloads and npm cache stay until this `Ready`, as a full try's do.
        if let Some(installed) = kept::finish_kept(self, &node)
            .await
            .map_err(TryError::in_setup)?
        {
            clean_up_finished(config, &self.ticket.agent);
            return Ok(installed);
        }
        // A try cut short left its npm cache behind; this one starts with none, so a run of
        // interrupted tries never holds more than one try's cache.
        empty_npm_cache(&config.data_dir);
        let node_needed = matches!(node, NodeChoice::Runtime { fetch: true, .. });
        self.check_disk(node_needed)?;
        let fetched = files::fetch_all(self, &node, run)?;
        let installed = npm::install(self, &node, &fetched, run)
            .await
            .map_err(TryError::in_setup)?;
        files::clean_up(config, fetched.downloaded());
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
        let python_needed = crate::python::needed(config);
        let need = config
            .snapshot
            .need_bytes(
                &self.ticket.agent,
                config.platform,
                node_needed,
                python_needed,
            )
            .unwrap_or(0);
        (need, disk::free_bytes(&config.data_dir).ok())
    }

    /// Sends `state` unless the job is paused; `false` when it is.
    pub(crate) fn emit(&self, state: InstallState, ends: bool) -> bool {
        self.shared.emit_job(&self.ticket, state, None, ends)
    }

    /// Sends the agent's `Ready`, after `tries` tries. When this platform has Python and it is
    /// not installed, the job goes on to install it under the same locks ([`python::step`]),
    /// and ends with Python's state instead; nothing Python meets changes the agent's `Ready`.
    async fn ready(&self, installed: Installed, tries: u32) {
        self.shared.set_tries(&self.ticket.agent, tries);
        let version = installed.version.clone();
        let state = InstallState::Ready { version };
        if !crate::python::needed(&self.config) {
            self.release_lock();
            self.shared
                .emit_job(&self.ticket, state, Some(installed), true);
            return;
        }
        if !self
            .shared
            .emit_job(&self.ticket, state, Some(installed), false)
        {
            return;
        }
        let Some(python) = python::step(self).await else {
            return;
        };
        self.release_lock();
        self.shared.end_python_job(&self.ticket, python);
    }

    /// Sends the job's last state, `Failed` of `kind`, with its log.
    fn fail(&self, kind: FailureKind, err: &TryError, run: &Run, user: Option<&FoundNode>) {
        self.release_lock();
        let mut log = run.log.clone();
        push_error(&mut log, err);
        finish_log(&mut log, run, kind);
        self.shared.set_tries(&self.ticket.agent, run.tries);
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

/// The clean-up after `Ready` ([`files::clean_up`]) for a `Ready` that fetched nothing itself:
/// a kept set-up renamed alone, or an install a job finds finished at its start.
/// Every package tarball the install may have put in `downloads/` goes, with the npm cache, and
/// Node's archive once the runtime is unpacked (another agent's install may still want it
/// otherwise). Only with `agents/.lock` held.
fn clean_up_finished(config: &AgentsConfig, agent: &AgentId) {
    let with_node = runtime_ready(config);
    files::clean_up(config, &files::install_downloads(config, agent, with_node));
}
