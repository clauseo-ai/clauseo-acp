//! The updater's boundary (decision record 0037): the [`Updater`] trait desktop implements over
//! `tauri-plugin-updater`, the values that cross it, and [`NoUpdater`], the default, which says
//! updates are managed by a package so no check ever runs.
//!
//! The host checks and downloads through the trait; it never installs. On install it shuts down
//! and hands the [`Downloaded`] value to desktop, which calls the plugin's `install` after the
//! host has stopped. `machine` holds the pure state machine that decides when each call happens.
//!
//! Every future the trait returns runs on the host's `LocalPool`: no tokio. Desktop runs the
//! plugin on its own runtime and hands back a future over a `futures::channel::oneshot`; dropping
//! that future cancels the work.

use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

pub(crate) mod machine;
mod marker;

pub use marker::read_update_marker;
pub(crate) use marker::{clear_marker, find_cached, write_marker};

/// Checks for and downloads the next release. `host` holds an `Arc<dyn Updater>`.
pub trait Updater: Send + Sync {
    /// How this copy of the app was installed; anything but [`InstallKind::CurrentUser`] means
    /// someone else updates it, and [`Updater::check`] is never called.
    fn install_kind(&self) -> InstallKind;

    /// Asks the release endpoint for a newer version: `Ok(None)` when there is none.
    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>>;

    /// Downloads `offer` into its folder under `Locations::updates_dir()` and checks its
    /// signature and signed version, posting the bytes received so far to `progress`.
    fn download(
        &self,
        offer: Offer,
        progress: ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>>;
}

/// How the running app was installed, which decides whether it may update itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallKind {
    /// Installed for the signed-in user only: it updates itself.
    CurrentUser,
    /// Installed for every user of the machine (Program Files on Windows): IT updates it.
    PerMachine,
    /// Its folder is not writable by the user (a macOS `.app`), so an update would ask for an
    /// administrator: someone else updates it.
    ReadOnlyLocation,
    /// A package manager updates it (Linux), or this build has no updater.
    Package,
}

impl InstallKind {
    /// Every value, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::CurrentUser,
        Self::PerMachine,
        Self::ReadOnlyLocation,
        Self::Package,
    ];
}

/// Whatever the [`Updater`] needs to carry from one call to the next (the plugin's `Update`):
/// the host only moves it. Cloning shares it; two handles are equal when they share one value.
#[derive(Clone)]
pub struct Opaque(Arc<dyn Any + Send + Sync>);

impl Opaque {
    /// Wraps `value`.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// The value inside, when it is a `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl fmt::Debug for Opaque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Opaque(..)")
    }
}

impl PartialEq for Opaque {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Opaque {}

/// A newer version the endpoint offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Its version, as `latest.json` writes it (`0.4.1`).
    pub version: String,
    /// What the [`Updater`] needs to download it, and later to install it.
    pub handle: Opaque,
}

/// A version downloaded and its signature checked, ready for desktop to install once the host
/// has shut down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// Its version.
    pub version: String,
    /// The installer, under `Locations::updates_dir()`.
    pub path: PathBuf,
    /// The handle of the [`Offer`] it came from, for the plugin's `install`.
    pub handle: Opaque,
}

/// A download a previous run left on disk (`<data_dir>/updates/<version>/`), which `auto` mode
/// installs at the next start once a check offers the same version again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedDownload {
    /// Its version.
    pub version: String,
    /// The installer.
    pub path: PathBuf,
}

/// Why a check or a download failed. The machine turns any of them into
/// `UpdateState::Failed` at the stage that was running; the text is for the log only.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdaterError {
    /// The endpoint or the download could not be reached, or answered an error.
    #[error("network: {0}")]
    Network(String),
    /// The download's signature or signed version did not verify.
    #[error("signature: {0}")]
    Signature(String),
    /// The download could not be written.
    #[error("io: {0}")]
    Io(String),
    /// Anything else the plugin reported.
    #[error("{0}")]
    Other(String),
}

/// Bytes received so far, and the download's size when the server said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes received so far (a running total, not the last chunk).
    pub received: u64,
    /// The whole download's size, when known.
    pub total: Option<u64>,
}

/// Where a download posts its [`Progress`]: a bounded channel to the actor. Posting never
/// blocks; when the channel is full, or the actor has gone, the value is dropped, and the next
/// one carries the newer total anyway.
#[derive(Debug)]
pub struct ProgressSink(mpsc::Sender<Progress>);

impl ProgressSink {
    /// A sink and the receiver the actor reads, holding at most about `capacity` values.
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<Progress>) {
        let (tx, rx) = mpsc::channel(capacity);
        (Self(tx), rx)
    }

    /// Posts `received` bytes of `total`, or drops it when the channel is full.
    pub fn post(&mut self, received: u64, total: Option<u64>) {
        // Full or closed: dropped on purpose (the actor is never waited on).
        let _dropped = self.0.try_send(Progress { received, total });
    }
}

/// `update.json` in the data folder (`Locations::update_marker()`): written just before an
/// install, read at the next start. JSON: `{"from":"0.4.0","to":"0.4.1","state":"installing"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateMarker {
    /// The version that started the install.
    pub from: String,
    /// The version being installed.
    pub to: String,
    /// Where the install stood when the marker was written.
    pub state: MarkerState,
}

/// The one state an [`UpdateMarker`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerState {
    /// The install was about to start.
    Installing,
}

/// The updater of a build with none, of tests and of `cli`: managed by a package, so the host
/// never checks and never touches the network.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoUpdater;

impl Updater for NoUpdater {
    fn install_kind(&self) -> InstallKind {
        InstallKind::Package
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        Box::pin(futures::future::ready(Ok(None)))
    }

    fn download(
        &self,
        _offer: Offer,
        _progress: ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>> {
        Box::pin(futures::future::ready(Err(UpdaterError::Other(
            "this build has no updater".to_owned(),
        ))))
    }
}

#[cfg(test)]
mod tests;
