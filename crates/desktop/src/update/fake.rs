//! The `e2e` build's fake updater (`E2E_FAKE_UPDATE=<version>`), for the update screens' specs:
//! installed per user, it offers that version at every check, at once, and its download
//! finishes at once with nothing written. Its download is never installed: the app exits as
//! usual once the host stops for it (`install.rs`).

use clauseo_host::{Downloaded, InstallKind, Offer, Opaque, ProgressSink, Updater, UpdaterError};
use futures::future::BoxFuture;

/// What the fake's offers and downloads carry: nothing a plugin could install.
struct FakeHandle;

/// Offers one version, never installs.
pub(crate) struct FakeUpdater {
    version: String,
}

impl FakeUpdater {
    /// The fake offering `version`.
    pub(crate) fn new(version: String) -> Self {
        Self { version }
    }
}

/// The size the fake's download says it has, in bytes.
const SIZE: u64 = 1_000_000;

impl Updater for FakeUpdater {
    fn install_kind(&self) -> InstallKind {
        InstallKind::CurrentUser
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        let offer = Offer {
            version: self.version.clone(),
            handle: Opaque::new(FakeHandle),
        };
        Box::pin(futures::future::ready(Ok(Some(offer))))
    }

    fn download(
        &self,
        offer: Offer,
        mut progress: ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>> {
        progress.post(SIZE, Some(SIZE));
        Box::pin(futures::future::ready(Ok(Downloaded {
            // No file: the host never reads it, and nothing installs it.
            path: std::path::PathBuf::from(&offer.version),
            version: offer.version,
            handle: offer.handle,
        })))
    }
}
