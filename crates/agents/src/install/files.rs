//! A try's files: Node's archive when Node is downloaded, then each package of the agent's
//! derived lock for this platform. Each comes from the network (through the mirror when one is
//! set) into `agents/downloads/`, or from the offline bundle with no network call, and is
//! checked against its digest. Progress covers them all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_process::Timer;

use super::{Job, NodeChoice, Run, TryError};
use crate::config::AgentsConfig;
use crate::download::firm::{BundleEntry, bundle_file, bundle_path, copy_from_bundle, mirror_url};
use crate::download::{FetchJob, Fetcher, Magic, part_path};
use crate::platform::Os;
use crate::state::InstallState;
use crate::write;

/// A package's size cap when the snapshot has no size row for the agent and platform (fixture
/// snapshots): 512 MiB.
const PACKAGE_CAP_FALLBACK: u64 = 512 * 1024 * 1024;

/// The least time between two `Downloading` events.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// A try's checked files.
#[derive(Debug)]
pub(crate) struct Fetched {
    /// Each checked tarball's absolute path, by its lock entry's path: the `file:` URLs of the
    /// lock `npm ci` reads.
    pub(crate) tarballs: BTreeMap<String, PathBuf>,
    /// Node's checked archive, when the runtime is to be unpacked.
    pub(crate) node_archive: Option<PathBuf>,
    /// The files this install put in `downloads/`, removed once it is `Ready`.
    downloaded: Vec<PathBuf>,
}

/// One file to have.
enum Source {
    Fetch(FetchJob),
    /// An npm tarball already checked, in the offline bundle.
    Bundle(PathBuf),
    /// Node's archive, or an npm tarball on a Windows share, in the offline bundle: copied into
    /// `downloads/` as it is hashed.
    Copy {
        from: PathBuf,
        /// The file's name in failure details.
        label: String,
        job: FetchJob,
    },
}

struct Item {
    source: Source,
    /// Its size when known before its request.
    known: Option<u64>,
    /// The lock entry's path when it is an npm tarball; `None` for Node's archive.
    package: Option<String>,
}

/// `<data_dir>/agents/downloads`.
pub(crate) fn downloads_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("downloads")
}

/// Fetches (or finds in the bundle) every file of the try, sending progress.
pub(super) fn fetch_all<T: Timer + Send + Sync>(
    job: &Job<T>,
    node: &NodeChoice,
    run: &mut Run,
) -> Result<Fetched, TryError> {
    let items = items(job, node)?;
    let config = &job.config;
    let estimate = config
        .snapshot
        .agent(&job.ticket.agent)
        .and_then(|entry| entry.sizes_for(config.platform))
        .map_or(0, |row| row.download_bytes);
    let mut progress = Progress {
        received: vec![0; items.len()],
        totals: items.iter().map(|item| item.known).collect(),
        is_package: items.iter().map(|item| item.package.is_some()).collect(),
        estimate,
        last: None,
    };
    progress.tick(job, run, true);
    let mut fetcher = None;
    let mut fetched = Fetched {
        tarballs: BTreeMap::new(),
        node_archive: None,
        downloaded: Vec::new(),
    };
    for (n, item) in items.into_iter().enumerate() {
        if job.cancel().is_cancelled() {
            return Err(TryError::cancelled());
        }
        let path = match item.source {
            Source::Bundle(path) => {
                progress.done(n, item.known.unwrap_or(0));
                path
            }
            Source::Copy {
                from,
                label,
                job: copy,
            } => {
                let size = copy_from_bundle(
                    &from,
                    &label,
                    &copy,
                    job.cancel(),
                    &mut |received, total| {
                        progress.update(n, received, total);
                        progress.tick(job, run, false);
                    },
                    &config.data_dir,
                )?;
                progress.done(n, size);
                fetched.downloaded.push(copy.dest.clone());
                copy.dest
            }
            Source::Fetch(fetch_job) => {
                if fetcher.is_none() {
                    fetcher = Some(Fetcher::new(
                        &config.install,
                        &job.ticket.agent,
                        job.system_proxy.as_deref(),
                        &config.data_dir,
                    )?);
                }
                let Some(client) = fetcher.as_ref() else {
                    return Err(TryError::retry("no HTTP client", config));
                };
                let size = client.fetch(&fetch_job, job.cancel(), &mut |received, total| {
                    progress.update(n, received, total);
                    progress.tick(job, run, false);
                })?;
                progress.done(n, size);
                fetched.downloaded.push(fetch_job.dest.clone());
                fetch_job.dest
            }
        };
        match item.package {
            Some(entry) => {
                fetched.tarballs.insert(entry, path);
            }
            None => fetched.node_archive = Some(path),
        }
    }
    progress.tick(job, run, true);
    Ok(fetched)
}

/// The try's files, Node's archive first.
fn items<T: Timer + Send + Sync>(job: &Job<T>, node: &NodeChoice) -> Result<Vec<Item>, TryError> {
    let config = &job.config;
    let data = &config.data_dir;
    let downloads = downloads_dir(data);
    let size_of = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.len());
    let mut items = Vec::new();
    if let NodeChoice::Runtime { fetch: true, .. } = node {
        let entry = config.snapshot.node();
        let Some(archive) = config.snapshot.node_archive(config.platform) else {
            return Err(TryError::retry(
                "the snapshot has no Node for this platform",
                config,
            ));
        };
        let fetch_job = FetchJob {
            url: entry.url(archive),
            expected: archive.expected().clone(),
            dest: downloads.join(archive.name()),
            magic: if archive.is_zip() {
                Magic::Zip
            } else {
                Magic::Gzip
            },
            max_bytes: archive.size(),
        };
        let source = match &config.install.bundle {
            Some(bundle) => Source::Copy {
                from: bundle_path(
                    bundle,
                    BundleEntry::Node(archive.name()),
                    archive.expected(),
                    archive.name(),
                    data,
                )?,
                label: archive.name().to_owned(),
                job: fetch_job,
            },
            None => Source::Fetch(fetch_job),
        };
        items.push(Item {
            source,
            known: Some(archive.size()),
            package: None,
        });
    }
    // A package's cap is the agent's whole download for the platform (every package together),
    // so it is a loose bound on any one tarball; it still stops a hostile mirror from filling the
    // disk.
    let package_cap = config
        .snapshot
        .agent(&job.ticket.agent)
        .and_then(|entry| entry.sizes_for(config.platform))
        .map_or(PACKAGE_CAP_FALLBACK, |row| row.download_bytes);
    for package in config.snapshot.packages(&job.ticket.agent, config.platform) {
        let short = package.name.rsplit('/').next().unwrap_or(&package.name);
        let label = format!("{short}-{}.tgz", package.version);
        let dest = downloads.join(format!("{}.tgz", package.integrity.hex()));
        let item = if let Some(bundle) = &config.install.bundle {
            let in_bundle = bundle.join("npm");
            // npm cannot read a `file:` tarball on a Windows share (see `is_unc`): copied first.
            if config.platform.os == Os::Win32 && is_unc(&in_bundle.to_string_lossy()) {
                let from = bundle_path(bundle, BundleEntry::Npm, &package.integrity, &label, data)?;
                Item {
                    known: size_of(&from),
                    source: Source::Copy {
                        from,
                        label,
                        job: FetchJob {
                            url: package.resolved.clone(),
                            expected: package.integrity.clone(),
                            dest,
                            magic: Magic::Gzip,
                            max_bytes: package_cap,
                        },
                    },
                    package: Some(package.path.clone()),
                }
            } else {
                let path = bundle_file(bundle, &package.integrity, &label, data)?;
                Item {
                    known: size_of(&path),
                    source: Source::Bundle(path),
                    package: Some(package.path.clone()),
                }
            }
        } else {
            let url = match &config.install.registry {
                Some(mirror) => {
                    mirror_url(&package.resolved, config.snapshot.registry(), mirror, data)?
                }
                None => package.resolved.clone(),
            };
            Item {
                known: size_of(&dest),
                source: Source::Fetch(FetchJob {
                    url,
                    expected: package.integrity.clone(),
                    dest,
                    magic: Magic::Gzip,
                    max_bytes: package_cap,
                }),
                package: Some(package.path.clone()),
            }
        };
        items.push(item);
    }
    Ok(items)
}

/// Bytes received and the total, per file.
struct Progress {
    received: Vec<u64>,
    /// Each file's size once known.
    totals: Vec<Option<u64>>,
    is_package: Vec<bool>,
    /// The snapshot's size of the agent's packages, standing in for those not sized yet.
    estimate: u64,
    /// When the last event went out.
    last: Option<Duration>,
}

impl Progress {
    fn update(&mut self, n: usize, received: u64, total: u64) {
        if let Some(slot) = self.received.get_mut(n) {
            *slot = received;
        }
        if total > 0
            && let Some(slot) = self.totals.get_mut(n)
        {
            *slot = Some(total);
        }
    }

    fn done(&mut self, n: usize, size: u64) {
        self.update(n, size, size);
    }

    /// Bytes received, and the total: every known size, plus what the estimate leaves for the
    /// packages not sized yet.
    fn sums(&self) -> (u64, u64) {
        let received: u64 = self.received.iter().sum();
        let known: u64 = self.totals.iter().flatten().sum();
        let unsized_package = self
            .totals
            .iter()
            .zip(&self.is_package)
            .any(|(total, package)| total.is_none() && *package);
        let total = if unsized_package {
            let packages_known: u64 = self
                .totals
                .iter()
                .zip(&self.is_package)
                .filter_map(|(total, package)| total.filter(|_| *package))
                .sum();
            known.saturating_add(self.estimate.saturating_sub(packages_known))
        } else {
            known
        };
        (received, total.max(received))
    }

    /// Sends `Downloading` when `force`d (the first and the last) or 250 ms after the last.
    fn tick<T: Timer + Send + Sync>(&mut self, job: &Job<T>, run: &mut Run, force: bool) {
        let (received, total) = self.sums();
        run.received = received;
        run.total = total;
        let now = job.config.clock.monotonic();
        let due = self
            .last
            .is_none_or(|last| now.saturating_sub(last) >= PROGRESS_EVERY);
        if force || due {
            self.last = Some(now);
            job.emit(InstallState::Downloading { received, total }, false);
        }
    }
}

/// After `Ready`: removes this install's downloads (never the bundle's files) and the npm
/// cache's contents. A file that will not go is left; the next install reuses or replaces it.
/// The cache goes only when no link stands at `agents` or `npm-cache`.
pub(super) fn clean_up(config: &AgentsConfig, fetched: &Fetched) {
    for path in &fetched.downloaded {
        drop(write::remove_file(path));
        drop(write::remove_file(&part_path(path)));
    }
    let cache = npm_cache(&config.data_dir);
    if write::real_dirs_under(&config.data_dir, &cache).unwrap_or(false) {
        drop(write::remove_dir_all(&cache));
    }
}

/// Whether the Windows path `path` is on a share: `\\server\share\…`, `\\?\UNC\…` or
/// `\\.\UNC\…`, with `\` or `/`. npm 11 on Windows reads a `file:` tarball on a share from the
/// wrong path (its lock loader turns `file:////server/share/a.tgz` into
/// `file:\\server\share\a.tgz`, which its fetcher then reads as `C:\server\share\a.tgz`), so such
/// a bundle tarball is copied to `downloads/` first. A drive path, `\\?\C:\…` included, is not a
/// share. A plain string function, so its rule is tested on every OS.
pub(crate) fn is_unc(path: &str) -> bool {
    let path = path.replace('/', "\\");
    let upper = path.to_ascii_uppercase();
    if let Some(rest) = upper
        .strip_prefix(r"\\?\")
        .or_else(|| upper.strip_prefix(r"\\.\"))
    {
        return rest.starts_with(r"UNC\");
    }
    path.starts_with(r"\\")
}

/// Whether an install must refuse the data folder `data_dir` on `os`: a Windows share
/// ([`is_unc`]), whose tarballs' `file:` URLs npm 11 reads from the wrong path, so no retry
/// can help. A plain function, so its rule is tested on every OS.
pub(crate) fn data_dir_on_share(data_dir: &str, os: Os) -> bool {
    os == Os::Win32 && is_unc(data_dir)
}

/// `<data_dir>/agents/npm-cache`.
pub(crate) fn npm_cache(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("npm-cache")
}

#[cfg(test)]
mod tests;
