//! Which updater this build gives the host (decision record 0037): none, the plugin over
//! `tauri.conf.json`'s endpoints and key, or in the `e2e` build only, the plugin over a test
//! release trusting the throwaway test key, or a fake.
//!
//! The test key is compiled into the `e2e` build alone ([`TEST_KEY`]); [`TRUSTS_TEST_KEY`] is
//! false in every other build, and a release build reads none of the `E2E_*` variables.

#[cfg(not(target_os = "linux"))]
use std::path::PathBuf;
#[cfg(not(target_os = "linux"))]
use std::sync::Arc;

#[cfg(not(target_os = "linux"))]
use clauseo_host::Updater;
#[cfg(not(target_os = "linux"))]
use tauri::{AppHandle, Runtime};
use url::Url;

/// The throwaway test key's public half (the `.pub` file in desktop's `release` folder), which
/// signs test releases (0035's `test` mode). Only the `e2e` build has it: a release build never
/// trusts it.
#[cfg(feature = "e2e")]
pub(super) const TEST_KEY: Option<&str> = Some(include_str!("../../release/updater-test.pub"));
/// The throwaway test key's public half: none outside the `e2e` build.
#[cfg(not(feature = "e2e"))]
pub(super) const TEST_KEY: Option<&str> = None;

/// Whether this build trusts the test key: the `e2e` build only. [`choose`] takes the test
/// endpoint only when it does.
pub(super) const TRUSTS_TEST_KEY: bool = TEST_KEY.is_some();

/// The `e2e` build's update endpoint: a test release's `latest.json`, signed with the test key.
#[cfg(feature = "e2e")]
const ENDPOINT_VAR: &str = "E2E_UPDATE_ENDPOINT";

/// The `e2e` build's fake update: a version the fake offers at once and never installs.
#[cfg(feature = "e2e")]
const FAKE_VAR: &str = "E2E_FAKE_UPDATE";

/// Which build this is, as the updater sees it, the first that applies. (A Linux build never
/// gets here: it updates through its package manager, and `update.rs` gives its host none.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flavour {
    /// The `e2e` build, debug or not.
    E2e,
    /// Any other debug build.
    Debug,
    /// A release build.
    Release,
}

/// What decides the updater, besides the `E2E_*` variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Build {
    /// Which build.
    pub(super) flavour: Flavour,
    /// Whether the build trusts the test key ([`TRUSTS_TEST_KEY`]).
    pub(super) trusts_test_key: bool,
    /// Whether `tauri.conf.json` names at least one endpoint.
    pub(super) endpoints: bool,
}

/// The `e2e` build's variables, as read; empty in every other build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct E2eEnv {
    /// `E2E_FAKE_UPDATE`.
    pub(super) fake: Option<String>,
    /// `E2E_UPDATE_ENDPOINT`.
    pub(super) endpoint: Option<String>,
}

/// The updater a build gives the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Choice {
    /// None: the host keeps `NoUpdater`, `managed` by `package`, and never checks.
    None,
    /// The plugin, over `tauri.conf.json`'s endpoints and pubkey.
    Config,
    /// The plugin, over this test release's endpoint, trusting the test key (`e2e` only).
    TestRelease(Url),
    /// The fake, offering this version (`e2e` only).
    Fake(String),
}

/// Picks the updater. The `e2e` build takes one only from its variables, the fake first, so a
/// test run never reaches the real endpoint; any other build takes the config's endpoints, in a
/// release build only, and only when there is one.
pub(super) fn choose(build: Build, env: E2eEnv) -> Choice {
    match build.flavour {
        Flavour::E2e => {
            if let Some(version) = env.fake.filter(|version| !version.trim().is_empty()) {
                return Choice::Fake(version.trim().to_owned());
            }
            match env.endpoint.and_then(|url| Url::parse(url.trim()).ok()) {
                Some(url) if build.trusts_test_key => Choice::TestRelease(url),
                _ => Choice::None,
            }
        }
        Flavour::Release if build.endpoints => Choice::Config,
        // A debug build, or a release build with no endpoint (until R23 PR 6).
        Flavour::Debug | Flavour::Release => Choice::None,
    }
}

/// Whether `config`'s `plugins.updater.endpoints` holds at least one entry.
pub(super) fn has_endpoints(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("endpoints"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|endpoints| !endpoints.is_empty())
}

/// The `e2e` build's variables.
#[cfg(feature = "e2e")]
pub(super) fn e2e_env() -> E2eEnv {
    E2eEnv {
        fake: std::env::var(FAKE_VAR).ok(),
        endpoint: std::env::var(ENDPOINT_VAR).ok(),
    }
}

/// No variable is read outside the `e2e` build.
#[cfg(not(feature = "e2e"))]
pub(super) fn e2e_env() -> E2eEnv {
    E2eEnv::default()
}

/// The updater this build gives the host, or none ([`choose`]).
#[cfg(not(target_os = "linux"))]
pub(super) fn updater<R: Runtime>(
    app: &AppHandle<R>,
    updates_dir: PathBuf,
) -> Option<Arc<dyn Updater>> {
    let flavour = if cfg!(feature = "e2e") {
        Flavour::E2e
    } else if cfg!(debug_assertions) {
        Flavour::Debug
    } else {
        Flavour::Release
    };
    let build = Build {
        flavour,
        trusts_test_key: TRUSTS_TEST_KEY,
        endpoints: has_endpoints(app.config()),
    };
    let source = match choose(build, e2e_env()) {
        Choice::None => return None,
        Choice::Config => super::plugin::Source::Config,
        Choice::TestRelease(url) => super::plugin::Source::Endpoint {
            url,
            pubkey: TEST_KEY?.trim().to_owned(),
        },
        #[cfg(feature = "e2e")]
        Choice::Fake(version) => return Some(Arc::new(super::fake::FakeUpdater::new(version))),
        #[cfg(not(feature = "e2e"))]
        Choice::Fake(_) => return None,
    };
    let kind = super::install_kind::detect();
    Some(Arc::new(super::plugin::PluginUpdater::new(
        app.clone(),
        source,
        kind,
        updates_dir,
    )))
}
