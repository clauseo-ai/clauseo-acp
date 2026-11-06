//! The paid data connectors in the app (R24, PR5): the host's connector wiring and the case-law
//! relays, built on the host's start thread from the data folder.
//!
//! - The credential lives in the OS keychain (`KeyringStore`); the state file and the day count
//!   in `<data>/connectors/`.
//! - The connector's HTTPS goes the way the host's own does (`clauseo_sandbox::upstream_for`):
//!   straight, or through the firm's proxy as a `ProxyChoice::Fixed`, which prints no user info.
//!   The route is asked before each request (`ProxyChoice::Resolve`), never on the start
//!   thread: the lookup can wait up to 10 s on a WPAD script, and its own 5-minute cache then
//!   decides when the route is looked up again. A proxy setting the sandbox refuses fails each
//!   request, rather than going around it.
//! - The sign-in page opens with `open_url_unconfirmed` (https only, no confirm): the host's own
//!   link, never a window's or an agent's. The pay link (`OpenPayLink`) opens through
//!   `open_external`'s native confirm and gate ([`open_pay_link`]).
//! - The day cap's local day comes from the OS's time zone (`local_day.rs`).
//!
//! Anything that fails leaves the connectors out with one log line: every connector command is
//! then refused and no session gets case law.

mod local_day;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_connectors::{
    CASE_LAW, Config, Connectors, KeyringStore, Origin, ProxyChoice, ProxyRefused, ProxyResolver,
    SecretStore,
};
use clauseo_host::HostConfig;
use clauseo_host::SystemClock;
use clauseo_host::connectors::{
    ConnectorEvents, ConnectorOpener, ConnectorsWiring, RelayConfig, Relays,
};
use clauseo_sandbox::{Route, Upstream, upstream_for};
use tauri::{AppHandle, Runtime};

use clauseo_protocol::{ErrorCode, HostError, Language};
use tauri::Manager as _;

use crate::bridge::Bridge;
use crate::open::{
    LinkConfirm, NativeLinkConfirm, NativeOpener, OpenGates, Opener, open_url_unconfirmed,
};

pub(crate) use local_day::OsLocalDay;

/// The connectors' folder in the data folder.
const FOLDER: &str = "connectors";

/// `config` with the connectors and the case-law relays, or unchanged (logged) when they cannot
/// be built.
pub(crate) fn with_connectors<R: Runtime>(
    config: HostConfig,
    app: AppHandle<R>,
    data_dir: &Path,
) -> HostConfig {
    let opener = Arc::new(LinkOpener(app));
    let Ok(store) = KeyringStore::new() else {
        tracing::warn!("no data connectors: the keychain could not be opened");
        return config;
    };
    match build(
        data_dir,
        opener,
        Arc::new(store),
        ProxyResolver::new(route_now),
    ) {
        Ok((wiring, relays)) => config.with_connectors(wiring).with_case_law_relays(relays),
        Err(why) => {
            tracing::warn!(why, "no data connectors: their commands are refused");
            config
        }
    }
}

/// The wiring and the relays under `data_dir`, the route left to `route` (not asked here).
fn build(
    data_dir: &Path,
    opener: Arc<dyn ConnectorOpener>,
    store: Arc<dyn SecretStore>,
    route: ProxyResolver,
) -> Result<(ConnectorsWiring, Relays), &'static str> {
    let folder = data_dir.join(FOLDER);
    let mut config = Config::production(user_agent(), folder.join("state.json"));
    config.proxy = ProxyChoice::Resolve(route);
    let connectors = Connectors::new(config, store)
        .map_err(|_unsaid| "the connectors' state could not be read")?;
    let connector = connectors
        .get(CASE_LAW.id)
        .ok_or("the case-law connector is not compiled in")?;
    let events = ConnectorEvents::new();
    let relays = Relays::new(RelayConfig {
        connector,
        origin: Origin::Production,
        day_file: folder.join("day.json"),
        clock: Arc::new(SystemClock),
        local_day: Arc::new(OsLocalDay),
        events: Arc::<ConnectorEvents>::clone(&events),
    });
    let wiring = ConnectorsWiring {
        connectors: Arc::new(connectors),
        events,
        opener,
    };
    Ok((wiring, relays))
}

/// `Clauseo/<version>`, sent on every connector request.
fn user_agent() -> String {
    format!("Clauseo/{}", env!("CARGO_PKG_VERSION"))
}

/// Whether the last route asked for was refused, so a run of refusals logs once.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// The route to the connector now, as the host's own HTTPS takes it; asked before each
/// connector request, off the start thread.
fn route_now() -> Result<ProxyChoice, ProxyRefused> {
    let route = proxy_choice();
    let refused = route.is_err();
    if REFUSED.swap(refused, Ordering::Relaxed) != refused
        && let Err(why) = &route
    {
        tracing::warn!(
            why,
            "the data connectors' requests fail until the proxy setting changes"
        );
    }
    route.map_err(|_why| ProxyRefused)
}

/// The route to the connector, or why there is none.
fn proxy_choice() -> Result<ProxyChoice, &'static str> {
    let url =
        url::Url::parse(CASE_LAW.mcp_url).map_err(|_unsaid| "the connector's URL did not parse")?;
    // The error is not logged: it can quote the proxy setting, password and all.
    let route =
        upstream_for(&Upstream::System, &url).map_err(|_unsaid| "the proxy setting is refused")?;
    proxy_for(route)
}

/// The `ProxyChoice` for a route.
fn proxy_for(route: Route) -> Result<ProxyChoice, &'static str> {
    match route {
        Route::Direct => Ok(ProxyChoice::Direct),
        Route::Http { host, port } => {
            // An IPv6 literal comes without brackets.
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host
            };
            url::Url::parse(&format!("http://{host}:{port}"))
                .map(ProxyChoice::Fixed)
                .map_err(|_unsaid| "the proxy's address did not parse")
        }
    }
}

/// The host's opener: the sign-in page with no confirm, the pay link behind the confirm.
struct LinkOpener<R: Runtime>(AppHandle<R>);

impl<R: Runtime> ConnectorOpener for LinkOpener<R> {
    fn open(&self, url: &str) -> Result<(), String> {
        open_link(&NativeOpener::new(self.0.clone()), url)
    }

    fn open_confirmed(&self, url: &str) -> Result<(), HostError> {
        let app = &self.0;
        let (Some(gates), Some(bridge)) = (app.try_state::<OpenGates>(), app.try_state::<Bridge>())
        else {
            return Err(HostError::new(
                ErrorCode::Internal,
                "the app is not ready to open links",
            ));
        };
        let confirm = NativeLinkConfirm::new(app.clone());
        let opener = NativeOpener::new(app.clone());
        tauri::async_runtime::block_on(async {
            let language = bridge.language().await;
            open_pay_link(
                &confirm,
                &opener,
                &gates.links,
                &crate::open::now,
                language,
                url,
            )
            .await
        })
    }
}

/// `OpenPayLink`'s open: the host's checked pay link, https only, through `open_external`'s
/// native confirm and gate (one confirm at a time, a pause after a decline). Declined, nothing
/// opens (`transport.unauthorized`).
pub(crate) async fn open_pay_link(
    confirm: &dyn LinkConfirm,
    opener: &dyn Opener,
    gate: &crate::open::gate::Gate,
    now: crate::open::Now<'_>,
    language: Language,
    url: &str,
) -> Result<(), HostError> {
    let link = crate::open::check_link(url)?;
    if !link.url.starts_with("https://") {
        return Err(HostError::new(
            ErrorCode::CommandInvalid,
            "a pay link is https only",
        ));
    }
    crate::open::open_external(confirm, opener, gate, now, language, &link).await
}

/// Opens `url` with no confirm, https only; the error names no URL.
fn open_link(opener: &dyn Opener, url: &str) -> Result<(), String> {
    open_url_unconfirmed(opener, url).map_err(|error| {
        tracing::warn!(code = ?error.code(), "the sign-in page did not open");
        "the browser did not open the sign-in page".to_owned()
    })
}

#[cfg(test)]
mod tests;
