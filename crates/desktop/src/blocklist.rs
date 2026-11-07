//! The block list's seams for the host (ADR 0053): the shared list in use and the release's
//! update settings, with the snapshot bundled at `<resource dir>/blocklist/`
//! (`blocklist-v1.txt.gz` and its `.minisig`). A build without the snapshot (a dev build)
//! starts unloaded and blocks nothing until the host's first fetch succeeds. Beside it, the
//! lawyer's allowed sites (PR 2): the views the host publishes them to, one per matter.

use std::sync::Arc;

use clauseo_blocklist::Current;
use clauseo_host::{AllowedSites, BlockListSeams, HostConfig};
use tauri::{AppHandle, Manager as _, Runtime};

/// The bundled snapshot's folder under the app's resource folder.
const BUNDLED_FOLDER: &str = "blocklist";

/// The list in use, shared with the host's updater, in Tauri's managed state: what R17 PR-D's
/// proxy wiring passes to `ProxyConfig::block`, so the proxy reads every list the host swaps in.
pub(crate) struct BlockListInUse(Arc<Current>);

/// The shared list itself; `Arc::clone` of it is what the proxy's config takes.
impl std::ops::Deref for BlockListInUse {
    type Target = Arc<Current>;

    fn deref(&self) -> &Arc<Current> {
        &self.0
    }
}

/// The lawyer's allowed sites (ADR 0053, PR 2), shared with the host, in Tauri's managed state:
/// R17 PR-D's proxy wiring passes `allowed.for_matter(<the launch's matter>)` to
/// `ProxyConfig::allowed`, so the launch's proxy reads every allow and revoke the host makes
/// for that matter, and none while policy forbids overrides.
pub(crate) struct AllowedSitesInUse(Arc<AllowedSites>);

/// The shared views; [`AllowedSites::for_matter`] gives one launch's.
impl std::ops::Deref for AllowedSitesInUse {
    type Target = Arc<AllowedSites>;

    fn deref(&self) -> &Arc<AllowedSites> {
        &self.0
    }
}

/// `config` with the block list: a fresh list in use (also put in `app`'s managed state as
/// [`BlockListInUse`]), fresh allowed sites' views (in it as [`AllowedSitesInUse`]), the bundled
/// folder when Tauri knows the resource folder, and the release's update settings.
pub(crate) fn with_blocklist<R: Runtime>(app: &AppHandle<R>, config: HostConfig) -> HostConfig {
    let bundled = match app.path().resource_dir() {
        Ok(folder) => Some(folder.join(BUNDLED_FOLDER)),
        Err(err) => {
            tracing::info!(%err, "no resource folder: no bundled block list");
            None
        }
    };
    let current = Arc::new(Current::default());
    app.manage(BlockListInUse(Arc::clone(&current)));
    let allowed = Arc::new(AllowedSites::default());
    app.manage(AllowedSitesInUse(Arc::clone(&allowed)));
    BlockListSeams::release(current, bundled)
        .with_allowed(allowed)
        .apply(config)
}
