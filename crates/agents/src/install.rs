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
//! Each try starts by emptying the agent's npm cache ([`empty_npm_cache`]), so tries cut short
//! never pile up its copies of the tarballs; except a try whose set-up is kept from one that
//! failed only its rename, which renames it alone (`kept`). Every way to `Ready` then removes
//! the install's downloads and its npm cache ([`files::clean_up`]); a failed try keeps both.
//! - A pause ends the job quietly where it is: `pause` has already sent `Paused`.
//!
//! Two agents install at once (decision 0033, "Two installs at once"), so a job holds the lock
//! files in `agents/` ([`Locks`], `write::AgentsLock`) for what it shares: `.lock` shared for
//! the whole job (an older Clauseo takes it exclusive), its agent's `.lock-<id>` exclusive for
//! the whole job, `.lock-node` exclusive while a try checks, fetches and unpacks the downloaded
//! Node and shared while its npm runs from it (a try that finds `.lock-node` held fetches no
//! Node and waits for the holder before its npm), and `.lock-python` while it installs Python.
//! Only `.lock-node` is ever waited for; every other lock is taken or the job stops, so no two
//! jobs wait on each other.
//!
//! Every wait and every npm run is raced against the job's [`Cancel`], and every event goes
//! through [`Shared::emit_job`], which drops it once the job is paused.

mod files;
mod kept;
mod locks;
mod npm;
mod python;
mod ready;
mod try_error;

pub(crate) use files::{empty_npm_cache, remove_legacy_shared, written};
#[cfg(test)]
pub(crate) use kept::MARKER;
pub(crate) use kept::marked_for;
pub(crate) use locks::Locks;
use try_error::Next;
pub(crate) use try_error::TryError;

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
use crate::write::{self, AgentsLock, Mode};

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
    /// The agent's turn lock: held for the whole job, so the agent's jobs run one after another.
    pub(crate) turn: Arc<Mutex<()>>,
    /// The OS's proxy, when a retry asked for it.
    pub(crate) system_proxy: Option<String>,
    /// The lock files the job holds while it works, each given up before the job's last
    /// event, so the next job (here or in another process) never meets them.
    pub(crate) locks: Locks,
}

/// The job thread's body: waits for its agent's turn, then works unless paused meanwhile.
pub(crate) fn run<T: Timer + Send + Sync>(job: &Job<T>) {
    let _turn = job.turn.lock().unwrap_or_else(PoisonError::into_inner);
    if job.ticket.cancel.is_cancelled() {
        return;
    }
    futures::executor::block_on(job.work());
    // Given up before the turn lock, so the next job here never finds them held.
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
    /// Checks the data folder and the bundle are absolute, then takes `agents/.lock` shared and
    /// the agent's `.lock-<id>` exclusive. Another process holding either (an older Clauseo
    /// installing anything, or another Clauseo installing this agent) fails the job at once: the
    /// single-instance plugin makes that rare, and a retry once it is done works.
    fn take_lock(&self) -> Result<(), TryError> {
        let config = &self.config;
        check_paths(config)?;
        let busy = |what: &str| {
            let detail =
                format!("another Clauseo is installing {what} now; try again once it is done");
            TryError::new(Next::Stop, &detail, config)
        };
        let lock = |lock, mode| {
            write::try_lock(&config.data_dir, lock, mode)
                .map_err(|err| TryError::io("locking the agents folder", &err, config))
        };
        let all = lock(AgentsLock::All, Mode::Shared)?.ok_or_else(|| busy("an agent"))?;
        let own = lock(AgentsLock::Agent(&self.ticket.agent), Mode::Exclusive)?
            .ok_or_else(|| busy(&format!("{}", self.ticket.agent)))?;
        self.locks.hold_job(all, own);
        Ok(())
    }

    /// Gives up every lock the job holds.
    fn release_lock(&self) {
        self.locks.release_all();
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
            let attempted = self.attempt(user.as_ref(), &mut run).await;
            // Whatever the try's end, it holds Node no longer.
            self.locks.release_node();
            let err = match attempted {
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
            None => self.runtime_node().await?,
        };
        // A finished set-up whose rename failed: only the rename runs again.
        // The earlier try's downloads and npm cache stay until this `Ready`, as a full try's do.
        if let Some(installed) = kept::finish_kept(self, &node)
            .await
            .map_err(TryError::in_setup)?
        {
            self.locks.release_node();
            clean_up_finished(config, &self.ticket.agent);
            return Ok(installed);
        }
        // A try cut short left its npm cache behind; this one starts with none, so a run of
        // interrupted tries never holds more than one try's cache.
        empty_npm_cache(&config.data_dir, &self.ticket.agent);
        let node_needed = matches!(node, NodeChoice::Runtime { fetch: true, .. });
        self.check_disk(node_needed)?;
        let fetched = files::fetch_all(self, &node, run)?;
        let installed = npm::install(self, &node, &fetched, run)
            .await
            .map_err(TryError::in_setup);
        // Node's archive goes with the clean-up only once no try holds Node.
        self.locks.release_node();
        let installed = installed?;
        files::clean_up(config, &self.ticket.agent, fetched.downloaded());
        Ok(installed)
    }

    /// The downloaded Node for this try (`.lock-node`, [`Locks`]). With the lock free, the
    /// runtime is checked under it, held exclusive, and when it is not usable this try keeps the
    /// lock and fetches and unpacks Node (`fetch`), so no other try fetches it twice or races
    /// its unpack. With the lock held (another try fetching Node, here or in another process, or
    /// an npm running from it) this try fetches no Node and waits for that holder before its npm
    /// ([`Job::use_node`]).
    async fn runtime_node(&self) -> Result<NodeChoice, TryError> {
        let config = &self.config;
        let dir = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
        let lock = locks::try_held(config, AgentsLock::Node, Mode::Exclusive)?;
        let Some(lock) = lock else {
            tracing::info!(
                agent = %self.ticket.agent,
                "agent install: another install holds the downloaded Node; this one fetches none"
            );
            return Ok(NodeChoice::Runtime { dir, fetch: false });
        };
        let fetch = !runtime_check(config, &*self.launcher, &*self.timer)
            .await
            .usable();
        if fetch {
            self.locks.hold_node(lock);
        }
        Ok(NodeChoice::Runtime { dir, fetch })
    }

    /// Unpacks Node's checked archive at `archive` into this try's runtime folder (`node`,
    /// fetched by this try under `.lock-node`, held exclusive since its check), then gives the
    /// lock up: right after Node's download, before the agent's own packages, so another try
    /// waits for Node alone. A failed unpack is the set-up's failure, as npm's is.
    pub(crate) fn unpack_node(
        &self,
        node: &NodeChoice,
        archive: &std::path::Path,
    ) -> Result<(), TryError> {
        let config = &self.config;
        let (NodeChoice::Runtime { dir, fetch: true }, Some(entry)) =
            (node, config.snapshot.node_archive(config.platform))
        else {
            return Err(npm::missing_archive(config));
        };
        node::install_runtime(
            archive,
            config.snapshot.node(),
            entry,
            dir,
            config.platform.os,
        )
        .map_err(|err| TryError::extract(&err, config).in_setup())?;
        self.locks.release_node();
        Ok(())
    }

    /// Holds `.lock-node` shared for this try's npm runs, waiting while another try holds it
    /// exclusive (fetching or unpacking Node), and gives up any exclusive hold this try took to
    /// fetch Node first (it is unpacked by then). Then the runtime must be complete: one another
    /// try was to fetch and did not is this try's error, and its next try fetches Node itself.
    pub(crate) async fn use_node(&self) -> Result<(), TryError> {
        let config = &self.config;
        self.locks.release_node();
        locks::wait_for(self, AgentsLock::Node, Mode::Shared, |file| {
            self.locks.hold_node(file);
        })
        .await?;
        if !runtime_ready(config) {
            self.locks.release_node();
            return Err(TryError::retry(
                "the downloaded Node another install was fetching is not ready",
                config,
            )
            .not_setup());
        }
        Ok(())
    }

    /// `DiskFull` when the free space at the data folder is below this try's need and every
    /// other running try's remaining need (`worker/space.rs`). A free space that cannot be
    /// read is not checked.
    fn check_disk(&self, node_needed: bool) -> Result<(), TryError> {
        let need = self.need(node_needed);
        crate::worker::space::check_space(
            &self.shared,
            &self.config,
            &self.ticket,
            need,
            node_needed,
        )
        .map_err(|(need, free)| {
            let free = free.unwrap_or(0);
            TryError::new(
                Next::DiskFull,
                &format!("{} MB free, {} MB needed", free / MB, need.div_ceil(MB)),
                &self.config,
            )
        })
    }

    /// The bytes this install needs, its agent's part and Python's (0 when Python is installed
    /// or this platform has none).
    fn need(&self, node_needed: bool) -> (u64, u64) {
        let config = &self.config;
        let need = |python| {
            config
                .snapshot
                .need_bytes(&self.ticket.agent, config.platform, node_needed, python)
                .unwrap_or(0)
        };
        let agent = need(false);
        let python = if crate::python::needed(config) {
            need(true).saturating_sub(agent)
        } else {
            0
        };
        (agent, python)
    }

    /// The bytes this install needs with every other running try's remaining need, and the
    /// bytes free: a `DiskFull` failure's figures.
    fn space(&self, node_needed: bool) -> (u64, Option<u64>) {
        let (agent, python) = self.need(node_needed);
        let need = agent
            .saturating_add(python)
            .saturating_add(crate::worker::space::others_left(
                &self.shared,
                &self.config,
                &self.ticket,
            ));
        (need, disk::free_for(&self.config))
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
        // Python is one for the computer: a job reaching `Ready` while another installs it
        // ends here, and that job reports Python.
        // Checked again once claimed: a job that finished Python just before needs no second.
        let config = &self.config;
        if !crate::python::needed(config)
            || !self.shared.claim_python(&self.ticket)
            || !crate::python::needed(config)
        {
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
/// otherwise) and `.lock-node` is free. Only with the agent's `.lock-<id>` held.
fn clean_up_finished(config: &AgentsConfig, agent: &AgentId) {
    let with_node = runtime_ready(config);
    files::clean_up(
        config,
        agent,
        &files::install_downloads(config, agent, with_node),
    );
}
