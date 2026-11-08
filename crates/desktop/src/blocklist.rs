//! The block list's seams for the host (ADR 0053): the shared list in use and the release's
//! update settings, with the snapshot bundled at `<resource dir>/blocklist/`
//! (`blocklist-v1.txt.gz` and its `.minisig`). A build without the snapshot (a dev build)
//! starts unloaded and blocks nothing until the host's first fetch succeeds. Beside it, the
//! lawyer's allowed sites (PR 2): the views the host publishes them to, one per matter, and the
//! OS keychain that keeps `allowed.json`'s hash, under the service [`KEYCHAIN_SERVICE`] (none on
//! Linux, where allows last one run). And the signed browser pins' bundled copy, at
//! `<resource dir>/browser-pins/` (`browser-pins-v1.json` and its `.minisig`), which the host
//! reads beside the copy its block list thread fetches; a build without it has none until the
//! first fetch.

use std::sync::Arc;

use clauseo_blocklist::Current;
use clauseo_connectors::KeyringStore;
use clauseo_host::{AllowedSites, BlockListSeams, BrowserPins, HostConfig};
use tauri::{AppHandle, Manager as _, Runtime};

/// The keychain service the allowed sites' hash is filed under, apart from the connectors'.
pub(crate) const KEYCHAIN_SERVICE: &str = "ai.clauseo.desktop.blocklist";

/// The bundled snapshot's folder under the app's resource folder.
const BUNDLED_FOLDER: &str = "blocklist";

/// The bundled browser pins' folder under the app's resource folder.
const BUNDLED_PINS_FOLDER: &str = clauseo_host::browser::pins::PINS_FOLDER;

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
/// folder when Tauri knows the resource folder, the release's update settings, and `secrets`,
/// the keychain the connectors use too (`connectors::secret_store`), under [`KEYCHAIN_SERVICE`].
/// With the release's browser pins too, their bundled copy's folder beside the list's.
pub(crate) fn with_blocklist<R: Runtime>(
    app: &AppHandle<R>,
    config: HostConfig,
    secrets: Option<KeyringStore>,
) -> HostConfig {
    let resources = match app.path().resource_dir() {
        Ok(folder) => Some(folder),
        Err(err) => {
            tracing::info!(%err, "no resource folder: no bundled block list or browser pins");
            None
        }
    };
    let bundled = resources.as_ref().map(|folder| folder.join(BUNDLED_FOLDER));
    let config = BrowserPins::release(
        resources
            .as_ref()
            .map(|folder| folder.join(BUNDLED_PINS_FOLDER)),
    )
    .apply(config);
    let current = Arc::new(Current::default());
    app.manage(BlockListInUse(Arc::clone(&current)));
    let allowed = Arc::new(AllowedSites::default());
    app.manage(AllowedSitesInUse(Arc::clone(&allowed)));
    let seams = BlockListSeams::release(current, bundled).with_allowed(allowed);
    match secrets {
        Some(secrets) => seams.with_secrets(Arc::new(secrets.with_service(KEYCHAIN_SERVICE))),
        None => seams,
    }
    .apply(config)
}
