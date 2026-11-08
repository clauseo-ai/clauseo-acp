//! The file binding at download time, end to end: the adapter's `download` against a release
//! served over HTTPS from 127.0.0.1 (clauseo-agents' `TestRegistry`, as a TLS-inspecting proxy
//! whose test CA the updater's client trusts), with a key made in the test. A file signed under
//! another name is refused and nothing is written under `updates/`; the right one is saved.

use std::path::Path;

use clauseo_agents::testing::TestRegistry;
use clauseo_host::{InstallKind, Offer, Opaque, ProgressSink, Updater as _, UpdaterError};
use serde_json::json;
use tauri_plugin_updater::UpdaterExt as _;

use super::super::file_binding::expected_update_file;
use super::super::plugin::{self, Pending, PluginUpdater, Source};
use super::file_binding::{TestKey, comment};
use super::plugin::{app, for_this_machine};
use crate::test_support::{Scratch, block_on};

/// The release's host: never resolved, the proxy answers every `CONNECT`.
const HOST: &str = "updates.clauseo.test";
/// The offered version.
const VERSION: &str = "0.4.1";
/// The artifact's path on the release.
const ARTIFACT: &str = "/Clauseo_0.4.1_x64-setup.exe";

/// What a download against a release whose artifact is signed as `signed_as` gave, the
/// `updates/` folder it saved into, and the requests the release saw.
struct Ran {
    result: Result<clauseo_host::Downloaded, UpdaterError>,
    scratch: Scratch,
    requests: Vec<clauseo_agents::testing::Request>,
}

impl Ran {
    fn updates(&self) -> std::path::PathBuf {
        self.scratch.path().join("updates")
    }
}

/// Serves a release of [`VERSION`] whose artifact is signed as `signed_as`, checks it through
/// an updater that trusts the release's CA, and downloads it with the adapter.
fn download_signed_as(test: &str, signed_as: &str) -> Ran {
    let registry = TestRegistry::start_tls_proxy().unwrap();
    let key = TestKey::new();
    let bytes = b"the installer's bytes".to_vec();
    let signature = key.sign(&bytes, &comment(signed_as, VERSION));
    let manifest = for_this_machine(json!({
        "version": VERSION,
        "notes": "",
        "pub_date": "2026-11-05T09:00:00Z",
        "platforms": {
            "darwin-aarch64-app": {
                "signature": signature,
                "url": format!("https://{HOST}{ARTIFACT}"),
            }
        }
    }));
    registry.route("/latest.json", serde_json::to_vec(&manifest).unwrap());
    registry.route(ARTIFACT, bytes);
    let proxy = registry.proxy_url();
    let ca = registry.ca_pem().unwrap();
    let url: url::Url = format!("https://{HOST}/latest.json").parse().unwrap();

    let app = app();
    let updater = app
        .handle()
        .updater_builder()
        .configure_client(move |builder| {
            plugin::client(builder)
                .proxy(reqwest::Proxy::all(&proxy).unwrap())
                .tls_certs_only([reqwest::Certificate::from_pem(ca.as_bytes()).unwrap()])
        })
        .endpoints(vec![url.clone()])
        .unwrap()
        .pubkey(key.public())
        .build()
        .unwrap();
    let update = tauri::async_runtime::block_on(updater.check())
        .unwrap()
        .expect("the release offers an update");
    let source = Source::Endpoint {
        url,
        pubkey: key.public(),
    };
    let scratch = Scratch::new(test, &[]);
    let adapter = PluginUpdater::new(
        app.handle().clone(),
        source.clone(),
        InstallKind::CurrentUser,
        scratch.path().join("updates"),
    );
    let offer = Offer {
        version: update.version.clone(),
        handle: Opaque::new(Pending { update, source }),
    };
    let (sink, _progress) = ProgressSink::channel(64);
    let result = block_on(adapter.download(offer, sink));
    Ran {
        result,
        scratch,
        requests: registry.requests(),
    }
}

/// Every entry under `folder`, recursively; none when it does not exist.
fn entries(folder: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in read {
        let path = entry.unwrap().path();
        names.push(path.display().to_string());
        names.extend(entries(&path));
    }
    names
}

/// Whether the artifact was fetched, inside the TLS tunnel.
fn fetched(ran: &Ran) -> bool {
    ran.requests
        .iter()
        .any(|request| request.tunnelled && request.path == ARTIFACT)
}

/// The other Windows architecture's installer, relative to the host running the test: x64's on
/// an arm64 host, arm64's otherwise. On a Windows arm64 runner arm64's is this platform's own.
const OTHER_ARCH_INSTALLER: &str = if cfg!(target_arch = "aarch64") {
    "Clauseo_0.4.1_x64-setup.exe"
} else {
    "Clauseo_0.4.1_arm64-setup.exe"
};

/// The sandbox tool and the other architecture's installer, each validly signed by the release
/// key at this version, are downloaded and then refused as the host's signature failure (the
/// host's `failed` state, stage `download`), and nothing lands where an accepted download is
/// saved: no version folder, no temporary file.
#[test]
fn a_download_signed_as_another_file_is_refused_and_never_written() {
    for other in ["clauseo-sandbox-setup.exe", OTHER_ARCH_INSTALLER] {
        assert_ne!(expected_update_file(VERSION).as_deref(), Some(other));
        let ran = download_signed_as("update-binding-refused", other);
        assert!(
            fetched(&ran),
            "the artifact was never fetched: {:?}",
            ran.requests
        );
        let line = if expected_update_file(VERSION).is_some() {
            "the signature is for another file than this platform's installer"
        } else {
            "this platform installs no update"
        };
        assert_eq!(
            ran.result.as_ref().err(),
            Some(&UpdaterError::Signature(line.to_owned())),
            "{other}"
        );
        assert_eq!(entries(&ran.updates()), Vec::<String>::new(), "{other}");
    }
}

/// The same release with the artifact signed as this platform's installer at this version is
/// saved as the one file of `updates/0.4.1/`. Linux installs no update, so there it is refused
/// like any other.
#[test]
fn a_download_signed_as_this_platforms_installer_is_saved() {
    let Some(expected) = expected_update_file(VERSION) else {
        let ran = download_signed_as("update-binding-linux", "Clauseo_0.4.1_x64-setup.exe");
        assert!(matches!(ran.result, Err(UpdaterError::Signature(_))));
        assert_eq!(entries(&ran.updates()), Vec::<String>::new());
        return;
    };
    let ran = download_signed_as("update-binding-accepted", &expected);
    assert!(fetched(&ran));
    let downloaded = ran.result.as_ref().unwrap();
    assert_eq!(downloaded.version, VERSION);
    assert_eq!(
        downloaded.path.parent(),
        Some(ran.updates().join(VERSION).as_path())
    );
    assert_eq!(
        std::fs::read(&downloaded.path).unwrap(),
        b"the installer's bytes"
    );
    assert_eq!(
        entries(&ran.updates()).len(),
        2,
        "{:?}",
        entries(&ran.updates())
    );
}
