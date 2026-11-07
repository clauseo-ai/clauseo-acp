//! The block list's seams for the host (ADR 0053): the shared list in use and the release's
//! update settings, with the snapshot bundled at `<resource dir>/blocklist/`
//! (`blocklist-v1.txt.gz` and its `.minisig`). A build without the snapshot (a dev build)
//! starts unloaded and blocks nothing until the host's first fetch succeeds.

use std::sync::Arc;

use clauseo_blocklist::Current;
use clauseo_host::{BlockListSeams, HostConfig};
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

/// `config` with the block list: a fresh list in use (also put in `app`'s managed state as
/// [`BlockListInUse`]), the bundled folder when Tauri knows the resource folder, and the
/// release's update settings.
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
    config.with_blocklist(BlockListSeams::release(current, bundled))
}
