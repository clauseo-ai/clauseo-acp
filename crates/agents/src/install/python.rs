//! The job's second step, after the agent is `Ready`: the Python runtime, when this platform has
//! it and it is not installed. Its archive comes from its URL (or the offline bundle's
//! `python/<archive name>`, copied and hashed into `downloads/`) through the same download path
//! as Node's, then [`crate::python::install_python_runtime`] unpacks, runs and seals it. Tries,
//! back-off and the offline wait follow the agent's (`install.rs`); every event is Python's
//! own, and nothing here touches the agent's state.

use std::path::Path;
use std::time::Duration;

use clauseo_process::Timer;

use super::{BACKOFF, Job, MB, Next, OFFLINE_WAIT, TRIES, TryError, files};
use crate::config::AgentsConfig;
use crate::disk;
use crate::download::firm::{BundleEntry, bundle_path, copy_from_bundle};
use crate::download::{FetchJob, Magic, part_path};
use crate::log::InstallLog;
use crate::python::{self, PythonError, RunFailure};
use crate::snapshot::PythonArchive;
use crate::state::{FailureKind, PythonFailure, PythonFailureKind, PythonState};
use crate::write::{self, ExtractError};

/// The least time between two Python `Downloading` events.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// How one Python try ended badly.
enum Stop {
    /// As an agent's try: offline, blocked, a full disk, a try to count, or paused.
    Try(TryError),
    /// The unpacked runtime would not run: no retry can help.
    Run(RunFailure),
}

impl From<TryError> for Stop {
    fn from(err: TryError) -> Stop {
        Stop::Try(err)
    }
}

/// Installs Python, its tries counted apart from the agent's: its last state, or `None` when the
/// job was paused or quit (a pause has already set it back to `Missing`).
pub(super) async fn step<T: Timer + Send + Sync>(job: &Job<T>) -> Option<PythonState> {
    let config = &job.config;
    let archive = config.snapshot.python_archive(config.platform)?;
    let mut log = InstallLog::new(&job.ticket.agent);
    log.push(format!("python: {}", archive.name()));
    let mut tries = 0_u32;
    loop {
        if job.cancel().is_cancelled() {
            return None;
        }
        let err = match attempt(job, archive).await {
            Ok(()) => {
                let version = config.snapshot.python().version().to_owned();
                return Some(PythonState::Ready { version });
            }
            Err(Stop::Run(RunFailure::Cancelled)) => return None,
            Err(Stop::Run(failure)) => {
                tries += 1;
                let (kind, blocked_by) = match failure {
                    RunFailure::Blocked(_, by) => (PythonFailureKind::RunBlocked, Some(by)),
                    _ => (PythonFailureKind::RunFailed, None),
                };
                log.push(crate::log::redact(&failure.to_string(), &config.data_dir));
                if let Some(by) = blocked_by {
                    log.push(format!("blocked by: {}", by.word()));
                }
                let mut state = failed(log, kind, tries);
                if let PythonState::Failed(failure) = &mut state {
                    failure.blocked_by = blocked_by;
                }
                return Some(state);
            }
            Err(Stop::Try(err)) => err,
        };
        match err.next {
            Next::Cancelled => return None,
            Next::Offline => {
                let mut waiting = log.clone();
                super::push_error(&mut waiting, &err);
                waiting.push(format!(
                    "result: {} after {tries} tries",
                    FailureKind::Offline.word()
                ));
                let state = PythonState::Waiting {
                    reason: FailureKind::Offline,
                    log: waiting,
                };
                if !job.shared.emit_python_job(&job.ticket, state) || !job.wait(OFFLINE_WAIT).await
                {
                    return None;
                }
            }
            Next::Blocked | Next::DiskFull | Next::Stop => {
                tries += 1;
                let kind = match err.next {
                    Next::Blocked => PythonFailureKind::Blocked,
                    Next::DiskFull => PythonFailureKind::DiskFull,
                    _ => PythonFailureKind::Failed,
                };
                super::push_error(&mut log, &err);
                return Some(failed(log, kind, tries));
            }
            Next::Retry => {
                tries += 1;
                super::push_error(&mut log, &err);
                let backoff = usize::try_from(tries - 1)
                    .ok()
                    .and_then(|n| BACKOFF.get(n).copied())
                    .filter(|_| tries < TRIES);
                let Some(backoff) = backoff else {
                    return Some(failed(log, PythonFailureKind::Failed, tries));
                };
                if !job.wait(backoff).await {
                    return None;
                }
            }
        }
    }
}

/// `Failed` of `kind`, with the log's `result:` line.
fn failed(mut log: InstallLog, kind: PythonFailureKind, tries: u32) -> PythonState {
    log.push(format!("result: {} after {tries} tries", kind.word()));
    PythonState::Failed(PythonFailure {
        kind,
        blocked_by: None,
        log,
    })
}

/// One try: the space check, the archive, then the unpack, the run and the receipt. On `Ready`
/// the archive is removed from `downloads/` (never the bundle's file), with the job's locks
/// still held, and any other Python runtime folder with it.
async fn attempt<T: Timer + Send + Sync>(
    job: &Job<T>,
    archive: &PythonArchive,
) -> Result<(), Stop> {
    let config = &job.config;
    check_disk(config, archive)?;
    let dest = files::downloads_dir(&config.data_dir).join(archive.name());
    fetch(job, archive, &dest)?;
    if !job
        .shared
        .emit_python_job(&job.ticket, PythonState::Installing)
    {
        return Err(TryError::cancelled().into());
    }
    let dir = python::runtime_dir(&config.data_dir, archive);
    let installed = python::install_python_runtime(
        &dest,
        config.snapshot.python(),
        archive,
        &dir,
        config.platform.os,
        &*job.launcher,
        &*job.timer,
        job.cancel(),
    )
    .await;
    match installed {
        Ok(()) => {}
        Err(PythonError::Run(failure)) => return Err(Stop::Run(failure)),
        Err(PythonError::Extract(ExtractError::Io(err))) => {
            return Err(TryError::io("unpacking Python failed", &err, config).into());
        }
        Err(PythonError::Extract(other)) => {
            return Err(TryError::retry(&other.to_string(), config).into());
        }
    }
    drop(write::remove_file(&dest));
    drop(write::remove_file(&part_path(&dest)));
    drop(python::remove_stale_runtimes(
        &config.data_dir,
        Some(archive.stem()),
    ));
    Ok(())
}

/// `DiskFull` when the free space at the data folder is below the archive, its unpacked size
/// and 10 %. A free space that cannot be read is not checked.
fn check_disk(config: &AgentsConfig, archive: &PythonArchive) -> Result<(), TryError> {
    let base = archive.size().saturating_add(archive.unpacked_bytes());
    let need = base.saturating_add(base.div_ceil(10));
    match disk::free_bytes(&config.data_dir) {
        Ok(free) if free < need => Err(TryError::new(
            Next::DiskFull,
            &format!("{} MB free, {} MB needed", free / MB, need.div_ceil(MB)),
            config,
        )),
        _ => Ok(()),
    }
}

/// Fetches the archive to `dest`, or copies it from the offline bundle, sending progress.
fn fetch<T: Timer + Send + Sync>(
    job: &Job<T>,
    archive: &PythonArchive,
    dest: &Path,
) -> Result<(), TryError> {
    let config = &job.config;
    let fetch_job = FetchJob {
        url: archive.url().to_owned(),
        expected: archive.expected().clone(),
        dest: dest.to_path_buf(),
        magic: Magic::Gzip,
        max_bytes: archive.size(),
    };
    let mut last: Option<Duration> = None;
    let mut progress = |received: u64, _total: u64| {
        let now = config.clock.monotonic();
        if last.is_none_or(|last| now.saturating_sub(last) >= PROGRESS_EVERY) {
            last = Some(now);
            let total = archive.size();
            let state = PythonState::Downloading {
                received: received.min(total),
                total,
            };
            job.shared.emit_python_job(&job.ticket, state);
        }
    };
    progress(0, archive.size());
    if let Some(bundle) = &config.install.bundle {
        let entry = BundleEntry::Python(archive.name());
        let from = bundle_path(
            bundle,
            entry,
            archive.expected(),
            archive.name(),
            &config.data_dir,
        )?;
        let cancel = job.cancel();
        copy_from_bundle(
            &from,
            archive.name(),
            &fetch_job,
            cancel,
            &mut progress,
            &config.data_dir,
        )?;
    } else {
        let client = files::new_fetcher(job)?;
        client.fetch(&fetch_job, job.cancel(), &mut progress)?;
    }
    Ok(())
}
