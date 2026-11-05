//! The Chrome for Testing download (S27, decision record 0029): after the lawyer agreed
//! (`HostCommand::InstallBrowser`), the host downloads Chrome for Testing once into
//! `<app data>/browsers/chrome-<version>/`, where [`super::chrome`] finds it at the next session.
//!
//! It runs on a thread of its own (`actor/browser_install.rs`), one at a time, and reports each
//! [`BrowserInstallState`] as it goes. In order:
//! 1. The machine: macOS (arm64, x64) or Windows x64 only; elsewhere `unsupported`.
//! 2. The disk: about 1 GB free in `<app data>/browsers` ([`MIN_FREE_BYTES`]), else `disk_space`.
//! 3. The version list ([`VERSIONS_URL`]), and from it Stable's `chrome` zip for this platform
//!    ([`manifest`]): a version of dotted digits and the one URL Google's layout gives it, else
//!    `host_refused`, all before any download.
//! 4. The zip, into `browsers/.partial-<version>-<random>/chrome.zip`, with progress; its size
//!    must equal the response's `Content-Length`, else `size_mismatch`.
//! 5. Unpacking: every entry name in the zip's directory is checked first ([`archive`]), then the
//!    system tool unpacks into `.partial-…/chrome/`, then the result is walked: every entry must
//!    sit inside, and no link may lead out. Else `unpack`.
//! 6. The code signature, checked and logged; it fails the download only while
//!    [`signature_required`] says so.
//! 7. `chrome/` is renamed to `browsers/chrome-<version>/`, walked again there, and the rest of
//!    `.partial-…` removed.
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
pub mod manifest;
pub mod system;
#[cfg(windows)]
mod windows;

pub use system::SystemTools;

/// The version list the download starts from, the source agent-browser's own `install` uses.
pub const VERSIONS_URL: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// The free space the download needs in the app data folder: the zip (about 150 MB) and what it
/// unpacks to (about 400 MB), side by side, with room to spare.
pub const MIN_FREE_BYTES: u64 = 1_000_000_000;

/// The prefix of a download's working folder, `browsers/.partial-<version>-<random>/`. The host
/// removes every leftover one at start ([`archive::sweep_partials`]).
pub const PARTIAL_PREFIX: &str = ".partial-";

/// The zip's name inside the working folder.
const ZIP_NAME: &str = "chrome.zip";

/// The folder inside the working folder that the zip unpacks into, renamed at the end.
const UNPACKED_NAME: &str = "chrome";

/// Whether a macOS download whose `codesign` check fails is refused. False until the by-hand
/// check in record 0029 shows Chrome for Testing is signed by Google (`EQHXZ8M8AV`).
pub const SIGNATURE_REQUIRED_MAC: bool = false;

/// Whether a Windows download whose Authenticode check of `chrome.exe` fails is refused. False
/// until the by-hand check in record 0029 shows Chrome for Testing is signed.
pub const SIGNATURE_REQUIRED_WINDOWS: bool = false;

/// Whether `platform`'s download is refused when its signature check fails.
pub fn signature_required(platform: Platform) -> bool {
    match platform {
        Platform::MacArm64 | Platform::MacX64 => SIGNATURE_REQUIRED_MAC,
        Platform::WindowsX64 => SIGNATURE_REQUIRED_WINDOWS,
        Platform::WindowsArm64 | Platform::Linux | Platform::Other => false,
    }
}

/// Why a download failed: the code the app shows copy for, and an English message for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The code.
    pub code: BrowserInstallFailure,
    /// For the log only.
    pub message: String,
}

impl Failure {
    /// A failure with `code` and `message`.
    pub fn new(code: BrowserInstallFailure, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The download was cancelled.
    pub fn cancelled() -> Self {
        Self::new(
            BrowserInstallFailure::Cancelled,
            "the download was cancelled",
        )
    }

    /// A file system call in the browsers folder failed: `unpack`.
    fn io(what: &str, error: &std::io::Error) -> Self {
        Self::new(BrowserInstallFailure::Unpack, format!("{what}: {error}"))
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
    /// use as its working folder.
    fn fetch_versions(&self, url: &str, work: &Path, cancel: &Cancel) -> Result<Vec<u8>, Failure>;

    /// Downloads `url` into the new file `zip`, calling `progress(bytes, total)` as it goes, at
    /// most about four times a second. Returns the response's `Content-Length`, `None` without
    /// one; the caller compares it with the file.
    fn download(
        &self,
        url: &str,
        zip: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure>;

    /// Unpacks `zip` into the empty folder `into`.
    fn unpack(&self, zip: &Path, into: &Path, cancel: &Cancel) -> Result<(), Failure>;

    /// Checks the code signature of the unpacked browser under `folder` (the macOS app bundle,
    /// or `chrome.exe` on Windows): `Err` with what the check said when it does not pass.
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
}

/// Runs the download, reporting `Downloading` and `Unpacking` to `report`, and returns the folder
/// it made, `browsers/chrome-<version>/`. On every failure the working folder is gone.
pub fn run(
    job: &Job,
    tools: &dyn InstallTools,
    cancel: &Cancel,
    report: &mut dyn FnMut(BrowserInstallState),
) -> Result<PathBuf, Failure> {
    let platform = job
        .platform
        .cft_name()
        .ok_or_else(|| Failure::new(BrowserInstallFailure::Unsupported, unsupported(job)))?;
    let browsers = job.app_data.join(BROWSERS_DIR);
    write::create_dir_all(&job.app_data)
        .map_err(|error| Failure::io("making the app data folder", &error))?;
    write::create_private_dirs_under(&job.app_data, &browsers)
        .map_err(|error| Failure::io("making the browsers folder", &error))?;
    match tools.free_bytes(&browsers) {
        Ok(free) if free < MIN_FREE_BYTES => {
            return Err(Failure::new(
                BrowserInstallFailure::DiskSpace,
                format!("{free} bytes free, {MIN_FREE_BYTES} needed"),
            ));
        }
        Ok(_) => {}
        // An unknown free space does not stop the download: a full disk fails it later.
        Err(error) => tracing::warn!(%error, "the free disk space was not read"),
    }
    cancel.check()?;
    manifest::check_versions_url(VERSIONS_URL)?;
    let body = tools.fetch_versions(VERSIONS_URL, &browsers, cancel)?;
    let download = manifest::stable_download(&body, platform)?;
    cancel.check()?;
    let partial = browsers.join(format!(
        "{PARTIAL_PREFIX}{}-{}",
        download.version, job.random
    ));
    write::create_private_dir(&partial)
        .map_err(|error| Failure::io("making the working folder", &error))?;
    let done = in_partial(job, tools, cancel, report, &download, &partial);
    // Whatever is left of the working folder goes, after success too (the zip).
    if let Err(error) = write::remove_tree(&partial) {
        tracing::warn!(%error, folder = %partial.display(), "a working folder was not removed");
    }
    done
}

/// The steps inside the working folder `partial`: download, check, unpack, walk, sign, rename.
fn in_partial(
    job: &Job,
    tools: &dyn InstallTools,
    cancel: &Cancel,
    report: &mut dyn FnMut(BrowserInstallState),
    download: &manifest::Download,
    partial: &Path,
) -> Result<PathBuf, Failure> {
    let zip = partial.join(ZIP_NAME);
    let mut progress = |bytes, total| {
        report(BrowserInstallState::Downloading {
            bytes: Count::clamped(bytes),
            total: Count::clamped(total),
        });
    };
    let length = tools.download(&download.url, &zip, cancel, &mut progress)?;
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
    let signed = tools.verify_signature(job.platform, &unpacked, cancel);
    cancel.check()?;
    check_signature(signature_required(job.platform), signed)?;
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
            tracing::warn!(%error, folder = %target.display(), "a refused browser was not removed");
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
            tracing::info!("the Chrome for Testing signature check passed");
            Ok(())
        }
        Err(said) if required => Err(Failure::new(BrowserInstallFailure::Signature, said)),
        Err(said) => {
            tracing::warn!(%said, "the Chrome for Testing signature check failed; not required");
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
