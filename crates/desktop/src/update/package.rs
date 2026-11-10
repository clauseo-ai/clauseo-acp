//! Linux's updater: there is none, as with the host's `NoUpdater`, but a Linux install really
//! is updated by its package manager, so the host shows `managed` by `package` rather than
//! `unavailable`. It never offers an update and never touches the network.

use clauseo_host::{
    Downloaded, InstallKind, NoUpdater, Offer, ProgressSink, Updater, UpdaterError,
};
use futures::future::BoxFuture;

/// `NoUpdater`, installed by a package manager.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PackageManaged;

impl Updater for PackageManaged {
    fn install_kind(&self) -> InstallKind {
        InstallKind::Package
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        NoUpdater.check()
    }

    fn download(
        &self,
        offer: Offer,
        progress: ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>> {
        NoUpdater.download(offer, progress)
    }
}
