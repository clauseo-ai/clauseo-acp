//! The Chrome for Testing download (S27, decision record 0029): after the lawyer agreed
//! (`HostCommand::InstallBrowser`), the host downloads Chrome for Testing once into
//! `<app data>/browsers/chrome-<version>/`, where [`super::chrome`] finds it at the next session.
//!
//! There is no Clauseo manifest: the host reads Google's own list and takes Stable from it. The
//! same run serves the lawyer's download and the daily check while the app runs, which
//! downloads only when Stable is newer than the copy installed.
//!
//! It runs on a thread of its own (`actor/browser_install.rs`), one at a time, and reports each
//! [`BrowserInstallState`] as it goes. In order:
//! 1. The machine: macOS (arm64, x64) only; elsewhere `unsupported`. Never on Windows, where the
//!    guard's piped browser cannot start from the per-user folder it would sit in
//!    (`super::chrome`), whatever the app asks.
//! 2. The version list ([`VERSIONS_URL`]), and from it Stable's `chrome` zip for this platform
//!    ([`versions`]): a version of dotted digits and the one URL Google's layout gives it, else
//!    `host_refused`, all before any download.
//! 3. The guardrails ([`fresh`]): the record of Stables seen is updated; a Stable below this
//!    release's floor or below the copy installed is `host_refused` (never downgraded); a
//!    Stable equal to the copy installed ends the run as [`Outcome::Current`].
//! 4. The disk: about 1 GB free in `<app data>/browsers` ([`MIN_FREE_BYTES`]), else `disk_space`.
//! 5. The zip, into `browsers/.partial-<version>-<random>/chrome.zip`, with progress; its size
//!    must equal the response's `Content-Length` and be at most [`MAX_ZIP_BYTES`], else
//!    `size_mismatch`.
//! 6. Unpacking: every entry name in the zip's directory is checked first ([`archive`]), then the
//!    system tool unpacks into `.partial-…/chrome/`, then the result is walked: every entry must
//!    sit inside, and no link may lead out. Else `unpack`.
//! 7. Windows only: the Authenticode check, logged; it fails the download only while
//!    [`SIGNATURE_REQUIRED_WINDOWS`] says so. None on macOS: Chrome for Testing there is ad hoc
//!    signed, with no team id, so no signature check could ever be required of it (record 0029).
//!    HTTPS to Google's two pinned hosts, no redirect, the size checks and the zip checks are
//!    the integrity checks, and the browsers folder sits outside what any agent can write.
//! 8. `chrome/` is renamed to `browsers/chrome-<version>/`, walked again there, and the rest of
//!    `.partial-…` removed. Older copies go once no session runs them ([`prune`]).
//!
//! Every way out but the last removes `.partial-…`, and a cancel (the lawyer's, or the host
//! stopping) ends the running tool at once. The tools themselves sit behind [`InstallTools`]:
//! production uses curl, ditto or tar and the OS's signature check ([`system`]); tests use fakes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, Count};

use super::chrome::{BROWSERS_DIR, CFT_PREFIX, Platform};
use crate::write;

pub mod archive;
pub mod fresh;
pub mod prune;
pub mod system;
pub mod versions;
#[cfg(windows)]
mod windows;

pub use system::SystemTools;

/// The version list the download starts from, the source agent-browser's own `install` uses.
pub const VERSIONS_URL: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// The free space the download needs in the app data folder: the zip (about 200 MB) and what it
/// unpacks to (about 400 MB), side by side, with room to spare.
pub const MIN_FREE_BYTES: u64 = 1_000_000_000;

/// The prefix of a download's working folder, `browsers/.partial-<version>-<random>/`. The host
/// removes every leftover one at start ([`archive::sweep_partials`]).
pub const PARTIAL_PREFIX: &str = ".partial-";

/// The largest zip the host takes, the same cap curl's `--max-filesize` sets
/// (`system::CURL_FLAGS`): Chrome for Testing's Mac zips are about 200 MB.
pub const MAX_ZIP_BYTES: u64 = 419_430_400;

/// How long the lawyer's download waits, while offline, before it tries the network again.
pub const OFFLINE_RETRY: std::time::Duration = std::time::Duration::from_secs(60);

/// How many times in all the lawyer's download tries while the connection fails, before it stops
/// with `network`: three, as agent-browser's own `install` does.
pub const OFFLINE_TRIES: u32 = 3;

/// The zip's name inside the working folder.
const ZIP_NAME: &str = "chrome.zip";

/// The folder inside the working folder that the zip unpacks into, renamed at the end.
const UNPACKED_NAME: &str = "chrome";

/// Whether a Windows download whose Authenticode check of `chrome.exe` fails is refused. False
/// until the by-hand check in record 0029 shows Chrome for Testing is signed there.
///
/// There is no macOS counterpart, and there must never be one: Chrome for Testing on a Mac is
/// ad hoc signed, with no team id, so `codesign` can never tie it to Google and no signature
/// can be required of it. The integrity of a Mac download rests on HTTPS to Google's pinned
/// hosts with no redirect, the size checks, the zip checks, and the browsers folder sitting
/// outside what any agent can write (anything running as the lawyer could patch an ad hoc
/// signed app).
pub const SIGNATURE_REQUIRED_WINDOWS: bool = false;

/// Whether `platform`'s download gets a signature check at all: Windows x64 only.
pub fn signature_checked(platform: Platform) -> bool {
    matches!(platform, Platform::WindowsX64)
}

/// Why a download failed: the code the app shows copy for, and an English message for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The code.
    pub code: BrowserInstallFailure,
    /// For the log only.
    pub message: String,
    /// Whether the lawyer's download may try again after it ([`run_until_online`]): a
    /// `network` failure of the connection itself, and nothing else.
    pub again: bool,
}

impl Failure {
    /// A failure with `code` and `message`; a `network` one may be tried again.
    pub fn new(code: BrowserInstallFailure, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            again: code == BrowserInstallFailure::Network,
        }
    }

    /// The same failure, never tried again.
    #[must_use]
    pub fn last_try(self) -> Self {
        Self {
            again: false,
            ..self
        }
    }

    /// The download was cancelled.
    pub fn cancelled() -> Self {
        Self::new(
            BrowserInstallFailure::Cancelled,
            "the download was cancelled",
        )
    }

    /// A file system call in the browsers folder failed: `disk_space` when the disk is full,
    /// else `unpack`.
    fn io(what: &str, error: &std::io::Error) -> Self {
        let code = if error.kind() == std::io::ErrorKind::StorageFull {
            BrowserInstallFailure::DiskSpace
        } else {
            BrowserInstallFailure::Unpack
        };
        Self::new(code, format!("{what}: {error}"))
    }
}

/// Asks a running download to stop: the lawyer's `CancelBrowserInstall`, or the host stopping.
/// The tools look at it at least every 250 ms and end their child process when it is set.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag not yet set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets it.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether it is set.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// `Err(cancelled)` once it is set.
    pub fn check(&self) -> Result<(), Failure> {
        if self.is_cancelled() {
            Err(Failure::cancelled())
        } else {
            Ok(())
        }
    }
}

/// What the download needs from the machine. Production: [`SystemTools`]. Each call blocks the
/// install thread and returns `Err(cancelled)` soon after `cancel` is set, its child ended.
pub trait InstallTools: Send + Sync {
    /// The bytes free to this user on the volume that holds `folder`.
    fn free_bytes(&self, folder: &Path) -> std::io::Result<u64>;

    /// The body of `url`, the version list (at most 1 MiB). `work` is a folder the call may
    /// use as its working folder; `proxy` is `network.upstream_proxy`, when set.
    fn fetch_versions(
        &self,
        url: &str,
        work: &Path,
        proxy: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<u8>, Failure>;

    /// Downloads `url` into the new file `zip`, calling `progress(bytes, total)` as it goes, at
    /// most about four times a second. Returns the response's `Content-Length`, `None` without
    /// one; the caller compares it with the file. `proxy` as for [`Self::fetch_versions`].
    fn download(
        &self,
        url: &str,
        zip: &Path,
        proxy: Option<&str>,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure>;

    /// Unpacks `zip` into the empty folder `into`.
    fn unpack(&self, zip: &Path, into: &Path, cancel: &Cancel) -> Result<(), Failure>;

    /// Checks the code signature of the unpacked browser under `folder` (`chrome.exe`, Windows
    /// only): `Err` with what the check said when it does not pass.
    fn verify_signature(
        &self,
        platform: Platform,
        folder: &Path,
        cancel: &Cancel,
    ) -> Result<(), String>;
}

/// One download: the machine and the app data folder.
#[derive(Debug, Clone)]
pub struct Job {
    /// The machine the download is for.
    pub platform: Platform,
    /// The app data folder that holds `browsers/`.
    pub app_data: PathBuf,
    /// The working folder's random part, `.partial-<version>-<random>`.
    pub random: String,
    /// Now, in seconds since the Unix epoch, for the record of Stables seen.
    pub now: i64,
    /// `network.upstream_proxy` when set, which curl then uses for both fetches; `None`: curl's
    /// own default.
    pub proxy: Option<String>,
}

/// How a run that did not fail ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A new copy is in place, in this folder.
    Installed(PathBuf),
    /// The copy installed is Stable already: nothing was downloaded.
    Current,
}

/// [`run`] again after each failure of the connection itself, reporting `Offline` and waiting
/// `retry` first, until it gets through, `cancel` is set, or `tries` runs have failed so (the
/// last failure is returned): the lawyer's download, which waits out a dropped connection for a
/// while. Any other failure (TLS, a refused address, an HTTP error, a full disk) is returned at
/// once.
pub fn run_until_online(
    job: &Job,
    tools: &dyn InstallTools,
    cancel: &Cancel,
    report: &mut dyn FnMut(BrowserInstallState),
    (retry, tries): (std::time::Duration, u32),
) -> Result<Outcome, Failure> {
    let mut tried = 0;
    loop {
        tried += 1;
        match run(job, tools, cancel, report) {
            Err(failure) if failure.again && tried < tries => {
                tracing::info!(message = %failure.message, "offline; the download tries again");
                report(BrowserInstallState::Offline);
                wait(retry, cancel)?;
            }
            done => return done,
        }
    }
}

/// Waits `retry` in steps of 250 ms (the system tools' tick), looking at `cancel` before each:
/// `Err(cancelled)` once it is set.
fn wait(retry: std::time::Duration, cancel: &Cancel) -> Result<(), Failure> {
    let step = std::time::Duration::from_millis(250);
    let mut left = retry;
    loop {
        cancel.check()?;
        if left.is_zero() {
            return Ok(());
        }
        let this = left.min(step);
        futures::executor::block_on(async_io::Timer::after(this));
        left = left.saturating_sub(this);
    }
}

/// Runs the check and, when Stable is newer than the copy installed, the download, reporting
/// `Downloading` and `Unpacking` to `report`; returns the folder it made,
/// `browsers/chrome-<version>/`, or [`Outcome::Current`]. On every failure the working folder
/// is gone.
pub fn run(
    job: &Job,
    tools: &dyn InstallTools,
    cancel: &Cancel,
    report: &mut dyn FnMut(BrowserInstallState),
) -> Result<Outcome, Failure> {
    let platform = job
        .platform
        .cft_name()
        .ok_or_else(|| Failure::new(BrowserInstallFailure::Unsupported, unsupported(job)))?;
    let browsers = job.app_data.join(BROWSERS_DIR);
    write::create_dir_all(&job.app_data)
        .map_err(|error| Failure::io("making the app data folder", &error))?;
    write::create_private_dirs_under(&job.app_data, &browsers)
        .map_err(|error| Failure::io("making the browsers folder", &error))?;
    cancel.check()?;
    versions::check_versions_url(VERSIONS_URL)?;
    let body = tools.fetch_versions(VERSIONS_URL, &browsers, job.proxy.as_deref(), cancel)?;
    let download = versions::stable_download(&body, platform)?;
    let record = fresh::observe(
        fresh::read_record(&browsers).as_ref(),
        &download.version,
        job.now,
    );
    if let Err(error) = fresh::write_record(&browsers, &record) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "the Stable record was not written");
    }
    let binary = job.platform.cft_binary().unwrap_or_default();
    let installed: Vec<String> = fresh::working(&browsers, &binary)
        .into_iter()
        .map(|(version, _)| version)
        .collect();
    if !fresh::check_install(&download.version, &installed)? {
        tracing::info!(version = %download.version, "Chrome for Testing is Stable already");
        return Ok(Outcome::Current);
    }
    let behind = installed
        .last()
        .is_some_and(|version| fresh::too_far_behind(version, Some(&record)));
    match tools.free_bytes(&browsers) {
        Ok(free) if free < MIN_FREE_BYTES => {
            return Err(Failure::new(
                BrowserInstallFailure::DiskSpace,
                format!("{free} bytes free, {MIN_FREE_BYTES} needed"),
            ));
        }
        Ok(_) => {}
        // An unknown free space does not stop the download: a full disk fails it later.
        Err(error) => {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "the free disk space was not read"
            );
        }
    }
    cancel.check()?;
    let partial = browsers.join(format!(
        "{PARTIAL_PREFIX}{}-{}",
        download.version, job.random
    ));
    write::create_private_dir(&partial)
        .map_err(|error| Failure::io("making the working folder", &error))?;
    let done = in_partial(job, tools, cancel, report, (&download, behind), &partial);
    // Whatever is left of the working folder goes, after success too (the zip).
    if let Err(error) = write::remove_tree(&partial) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "a working folder was not removed");
    }
    done.map(Outcome::Installed)
}

/// The steps inside the working folder `partial`: download, check, unpack, walk, sign, rename.
fn in_partial(
    job: &Job,
    tools: &dyn InstallTools,
    cancel: &Cancel,
    report: &mut dyn FnMut(BrowserInstallState),
    (download, behind): (&versions::Download, bool),
    partial: &Path,
) -> Result<PathBuf, Failure> {
    let zip = partial.join(ZIP_NAME);
    let mut progress = |bytes, total| {
        report(BrowserInstallState::Downloading {
            bytes: Count::clamped(bytes),
            total: Count::clamped(total),
            behind,
        });
    };
    let proxy = job.proxy.as_deref();
    let length = tools.download(&download.url, &zip, proxy, cancel, &mut progress)?;
    check_size(&zip, length)?;
    cancel.check()?;
    report(BrowserInstallState::Unpacking);
    archive::check_zip(&zip)?;
    let unpacked = partial.join(UNPACKED_NAME);
    write::create_private_dir(&unpacked)
        .map_err(|error| Failure::io("making the unpack folder", &error))?;
    tools.unpack(&zip, &unpacked, cancel)?;
    cancel.check()?;
    archive::check_tree(&unpacked)?;
    let binary = job.platform.cft_binary().unwrap_or_default();
    if !std::fs::symlink_metadata(unpacked.join(&binary)).is_ok_and(|meta| meta.is_file()) {
        return Err(Failure::new(
            BrowserInstallFailure::Unpack,
            format!("the zip holds no {}", binary.display()),
        ));
    }
    if signature_checked(job.platform) {
        let signed = tools.verify_signature(job.platform, &unpacked, cancel);
        cancel.check()?;
        check_signature(SIGNATURE_REQUIRED_WINDOWS, signed)?;
    }
    let browsers = partial.parent().unwrap_or(partial);
    let target = browsers.join(format!("{CFT_PREFIX}{}", download.version));
    // A folder of the same version that `chrome::find` passed over (its binary gone), or
    // anything else at that name, is replaced; a link there is removed, never followed.
    let old = match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => write::remove_tree(&target),
        Ok(_) => write::remove_link(&target),
        Err(_) => Ok(()),
    };
    old.map_err(|error| Failure::io("removing the old folder", &error))?;
    write::rename_folder(&unpacked, &target)
        .map_err(|error| Failure::io("moving the browser into place", &error))?;
    // The links once more where they now live: the walk above ran under another name.
    if let Err(failure) = archive::check_tree(&target) {
        if let Err(error) = write::remove_tree(&target) {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "a refused browser was not removed"
            );
        }
        return Err(failure);
    }
    Ok(target)
}

/// The zip's size against the response's `Content-Length`.
fn check_size(zip: &Path, length: Option<u64>) -> Result<(), Failure> {
    let size = std::fs::symlink_metadata(zip)
        .map_err(|error| Failure::io("reading the zip's size", &error))?
        .len();
    match length {
        Some(length) if length > MAX_ZIP_BYTES => Err(Failure::new(
            BrowserInstallFailure::SizeMismatch,
            format!("the zip's Content-Length {length} is over the {MAX_ZIP_BYTES} byte cap"),
        )),
        Some(length) if length == size => Ok(()),
        Some(length) => Err(Failure::new(
            BrowserInstallFailure::SizeMismatch,
            format!("the zip is {size} bytes, its Content-Length {length}"),
        )),
        None => Err(Failure::new(
            BrowserInstallFailure::SizeMismatch,
            "the response had no Content-Length",
        )),
    }
}

/// Logs the signature check's result, and fails with `signature` when it failed and `required`.
pub fn check_signature(required: bool, signed: Result<(), String>) -> Result<(), Failure> {
    match signed {
        Ok(()) => {
            tracing::info!("the Chrome for Testing Authenticode check passed");
            Ok(())
        }
        Err(said) if required => Err(Failure::new(BrowserInstallFailure::Signature, said)),
        Err(_) => {
            tracing::warn!("the Chrome for Testing Authenticode check failed; not required");
            Ok(())
        }
    }
}

/// Why `job`'s machine gets no download.
fn unsupported(job: &Job) -> String {
    format!("no Chrome for Testing download on {:?}", job.platform)
}

#[cfg(test)]
mod tests;
