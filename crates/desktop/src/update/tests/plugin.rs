//! The plugin adapter (`plugin.rs`) and S20's `latest.json` fixtures through the plugin's own
//! types (done-when 2 of R23); the fixtures are described in `tests.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;

use clauseo_host::{Downloaded, InstallKind, Opaque, Updater as _, UpdaterError};
use serde_json::Value;
use tauri::test::{MockRuntime, mock_builder};
use tauri::{App, AppHandle};
use tauri_plugin_updater::{
    Error as PluginError, RemoteRelease, RemoteReleaseInner, UpdaterExt as _,
};

use super::super::plugin::{self, Pending, PluginUpdater, Source};
use super::{crate_dir, test_key_file};
use crate::rust_only::PluginRustOnly as _;
use crate::test_support::{STEP_WAIT, Scratch, block_on};

/// A fixture's bytes.
fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(crate_dir().join("src/update/fixtures").join(name)).unwrap()
}

/// A `latest.json` fixture, parsed.
fn fixture(name: &str) -> Value {
    serde_json::from_slice(&fixture_bytes(name)).unwrap()
}

/// The fixture key's public half.
fn fixture_key() -> String {
    String::from_utf8(fixture_bytes("fixture.pub"))
        .unwrap()
        .trim()
        .to_owned()
}

/// An endpoint no test requests.
const ENDPOINT: &str = "https://example.invalid/latest.json";

/// An app on the mock runtime with the app's checked context and the updater plugin.
fn app() -> App<MockRuntime> {
    mock_builder()
        .plugin_rust_only(tauri_plugin_updater::Builder::new().build())
        .build(crate::context::<MockRuntime>())
        .unwrap()
}

/// The plugin's updater trusting `pubkey`, looking up `target` (or this machine's own keys).
fn updater(
    app: &AppHandle<MockRuntime>,
    target: Option<&str>,
    pubkey: &str,
) -> tauri_plugin_updater::Updater {
    let builder = app
        .updater_builder()
        .endpoints(vec![ENDPOINT.parse().unwrap()])
        .unwrap()
        .pubkey(pubkey);
    match target {
        Some(target) => builder.target(target),
        None => builder,
    }
    .build()
    .unwrap()
}

/// Done-when 2: each platform `latest.json` lists gets its update, whose signature verifies
/// against the payload.
#[test]
fn the_fixture_offers_its_update_to_each_platform_it_lists() {
    let manifest = fixture("latest.json");
    let release: RemoteRelease = serde_json::from_value(manifest.clone()).unwrap();
    assert_eq!(release.version.to_string(), "0.4.1");
    let RemoteReleaseInner::Static { platforms } = &release.data else {
        panic!("not the static format");
    };
    assert_eq!(platforms.len(), 6);
    let app = app();
    for (key, entry) in platforms {
        let updater = updater(app.handle(), Some(key), &fixture_key());
        let update = updater.restore_update(manifest.clone()).unwrap().unwrap();
        assert_eq!(update.version, "0.4.1", "{key}");
        assert_eq!(update.download_url, entry.url, "{key}");
        assert_eq!(update.signature, entry.signature, "{key}");
        update.verify(&fixture_bytes("payload.bin")).unwrap();
    }
}

/// Done-when 2: a release with a key for another platform only is no update, not a failure.
#[test]
fn a_release_for_another_platform_only_is_no_update() {
    let app = app();
    let mac_only = fixture("latest-mac-only.json");
    for target in ["windows-x86_64-nsis", "windows-x86_64", "darwin-x86_64-app"] {
        let updater = updater(app.handle(), Some(target), &fixture_key());
        let error = updater.restore_update(mac_only.clone()).err().unwrap();
        assert!(
            matches!(error, PluginError::TargetNotFound(_)),
            "{target}: {error:?}"
        );
        assert_eq!(plugin::no_entry_is_no_update(error), Ok(None), "{target}");
    }
    // Linux has no entry in any release: this machine's own lookup finds none.
    #[cfg(target_os = "linux")]
    {
        let updater = updater(app.handle(), None, &fixture_key());
        let error = updater
            .restore_update(fixture("latest.json"))
            .err()
            .unwrap();
        assert!(
            matches!(error, PluginError::TargetsNotFound(_)),
            "{error:?}"
        );
        assert_eq!(plugin::no_entry_is_no_update(error), Ok(None));
    }
    // Any other failure stays one.
    let other = plugin::no_entry_is_no_update(PluginError::ReleaseNotFound);
    assert!(matches!(other, Err(UpdaterError::Network(_))), "{other:?}");
}

/// Done-when 2: a bad signature is refused, and so are changed bytes, a signature for another
/// version than the one offered, and the right signature checked with another key.
#[test]
fn a_bad_signature_is_refused() {
    let app = app();
    let target = Some("darwin-aarch64-app");
    let payload = fixture_bytes("payload.bin");
    let refused = |update: &tauri_plugin_updater::Update, bytes: &[u8]| {
        let error = update.verify(bytes).unwrap_err();
        let failure = plugin::failure(&error);
        assert!(matches!(failure, UpdaterError::Signature(_)), "{error:?}");
    };

    let bad = updater(app.handle(), target, &fixture_key())
        .restore_update(fixture("latest-bad-signature.json"))
        .unwrap()
        .unwrap();
    refused(&bad, &payload);

    let good = updater(app.handle(), target, &fixture_key())
        .restore_update(fixture("latest.json"))
        .unwrap()
        .unwrap();
    let mut changed = payload.clone();
    changed[0] ^= 1;
    refused(&good, &changed);

    let mut newer = fixture("latest.json");
    newer["version"] = "0.4.2".into();
    let newer = updater(app.handle(), target, &fixture_key())
        .restore_update(newer)
        .unwrap()
        .unwrap();
    refused(&newer, &payload);

    let other_key = updater(app.handle(), target, &test_key_file())
        .restore_update(fixture("latest.json"))
        .unwrap()
        .unwrap();
    refused(&other_key, &payload);
}

/// `latest.json` with this machine's own lookup keys added, so `Source` (which sets no target)
/// finds an entry on every OS the tests run on.
fn for_this_machine(mut manifest: Value) -> Value {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let entry = manifest["platforms"]["darwin-aarch64-app"].clone();
    let platforms = manifest["platforms"].as_object_mut().unwrap();
    for installer in ["", "-app", "-nsis", "-msi", "-appimage", "-deb", "-rpm"] {
        platforms.insert(format!("{os}-{arch}{installer}"), entry.clone());
    }
    manifest
}

/// The install rebuilds the update and verifies the file's bytes again before anything runs: a
/// changed file is refused, and a missing one is an I/O failure. (A good file is never handed
/// over here: the plugin would install it.)
#[test]
fn the_install_verifies_the_saved_file_again() {
    let app = app();
    let scratch = Scratch::new("update-install", &["updates/0.4.1"]);
    let source = Source::Endpoint {
        url: ENDPOINT.parse().unwrap(),
        pubkey: fixture_key(),
    };
    let update = source
        .updater(app.handle())
        .unwrap()
        .restore_update(for_this_machine(fixture("latest.json")))
        .unwrap()
        .unwrap();
    let mut changed = fixture_bytes("payload.bin");
    changed[0] ^= 1;
    let path = scratch.file("updates/0.4.1/Clauseo-update", &changed);
    let mut downloaded = Downloaded {
        version: "0.4.1".to_owned(),
        path,
        handle: Opaque::new(Pending { update, source }),
    };
    let refused = plugin::install(app.handle(), &downloaded);
    assert!(
        matches!(refused, Err(UpdaterError::Signature(_))),
        "{refused:?}"
    );

    downloaded.path = scratch.path().join("updates/0.4.1/gone");
    let missing = plugin::install(app.handle(), &downloaded);
    assert!(matches!(missing, Err(UpdaterError::Io(_))), "{missing:?}");

    downloaded.version = "0.4.0".to_owned();
    let other = plugin::install(app.handle(), &downloaded);
    assert!(
        matches!(other, Err(UpdaterError::Signature(_))),
        "{other:?}"
    );

    downloaded.handle = Opaque::new(());
    let foreign = plugin::install(app.handle(), &downloaded);
    assert!(
        matches!(foreign, Err(UpdaterError::Other(_))),
        "{foreign:?}"
    );
}

/// The config's source (R23 PR 6) builds the plugin's updater over `tauri.conf.json`'s endpoint
/// and key, and that endpoint is the release's `latest.json` on GitHub, over HTTPS.
#[test]
fn the_config_source_builds_over_the_release_endpoint() {
    let app = app();
    assert!(Source::Config.updater(app.handle()).is_ok());
    let conf: Value =
        serde_json::from_slice(&std::fs::read(crate_dir().join("tauri.conf.json")).unwrap())
            .unwrap();
    assert_eq!(
        conf["plugins"]["updater"]["endpoints"],
        serde_json::json!([
            "https://github.com/clauseo-ai/clauseo-acp/releases/latest/download/latest.json"
        ])
    );
}

/// A check that cannot connect is a network failure whose line names no address.
#[test]
fn a_check_that_cannot_connect_is_a_network_failure_naming_no_url() {
    let app = app();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let source = Source::Endpoint {
        url: format!("https://127.0.0.1:{port}/latest.json")
            .parse()
            .unwrap(),
        pubkey: fixture_key(),
    };
    let scratch = Scratch::new("update-check", &[]);
    let updater = PluginUpdater::new(
        app.handle().clone(),
        source,
        InstallKind::CurrentUser,
        scratch.path().join("updates"),
    );
    assert_eq!(updater.install_kind(), InstallKind::CurrentUser);
    let failed = block_on(updater.check()).unwrap_err();
    let UpdaterError::Network(line) = &failed else {
        panic!("not a network failure: {failed:?}");
    };
    assert!(
        !line.contains("127.0.0.1") && !line.contains("https"),
        "{line}"
    );
}

/// Every request is HTTPS only: a check of an `http://` endpoint fails before any connection
/// is made, though something listens there (the plugin itself allows such an endpoint in a debug
/// build, with a warning, and never checks a redirect's or a download's scheme). The client's
/// settings (`plugin::client`) refuse it on their own too.
#[test]
fn a_request_over_http_is_refused_before_it_connects() {
    let app = app();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/latest.json");
    let source = Source::Endpoint {
        url: url.parse().unwrap(),
        pubkey: fixture_key(),
    };
    let scratch = Scratch::new("update-http", &[]);
    let updater = PluginUpdater::new(
        app.handle().clone(),
        source,
        InstallKind::CurrentUser,
        scratch.path().join("updates"),
    );
    let failed = block_on(updater.check()).unwrap_err();
    assert!(matches!(failed, UpdaterError::Network(_)), "{failed:?}");
    assert_eq!(
        listener.accept().map(|_accepted| ()).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "a connection reached the listener"
    );

    let client = plugin::client(reqwest::Client::builder()).build().unwrap();
    let refused = tauri::async_runtime::block_on(client.get(&url).send()).unwrap_err();
    assert!(refused.is_builder(), "{refused:?}");
    assert_eq!(
        listener.accept().map(|_accepted| ()).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "a connection reached the listener"
    );
}

/// A download's temporary file has a name of its own each time, in `updates/` and never read as
/// a version's folder, so two downloads of one version never share one.
#[test]
fn each_download_writes_a_temporary_file_of_its_own() {
    let first = plugin::temp_name("0.4.1");
    let second = plugin::temp_name("0.4.1");
    assert_ne!(first, second);
    for name in [&first, &second] {
        assert!(
            name.starts_with(".0.4.1.") && name.rsplit('.').next() == Some("part"),
            "{name}"
        );
        assert!(!name.contains('/') && !name.contains('\\'), "{name}");
    }
}

/// Dropping the future the host holds aborts the task on Tauri's runtime.
#[test]
fn dropping_the_future_aborts_its_task() {
    struct Hangs(std::sync::mpsc::Sender<()>);
    let (started, task) = std::sync::mpsc::channel();
    let future = plugin::on_runtime(async move {
        let hangs = Hangs(started);
        hangs.0.send(()).unwrap();
        futures::future::pending::<Result<(), UpdaterError>>().await
    });
    // Polled on the runtime without the host waiting.
    task.recv_timeout(STEP_WAIT).unwrap();
    drop(future);
    assert_eq!(
        task.recv_timeout(STEP_WAIT),
        Err(RecvTimeoutError::Disconnected)
    );

    assert_eq!(block_on(plugin::on_runtime(async { Ok(7) })), Ok(7));
}

/// The download is saved as the one file of `updates/<version>/`, whatever was there, with no
/// temporary file left beside it; a version that is not plain numbers writes nothing.
#[test]
fn a_download_is_saved_whole_as_the_one_file_of_its_folder() {
    let scratch = Scratch::new("update-save", &["updates/0.4.1"]);
    let _old = scratch.file("updates/0.4.1/half", b"half");
    let _other = scratch.file("updates/0.4.1/other", b"other");
    let updates = scratch.path().join("updates");
    let path = plugin::save_download(&updates, "0.4.1", b"installer").unwrap();
    assert_eq!(path.parent(), Some(updates.join("0.4.1").as_path()));
    assert_eq!(std::fs::read(&path).unwrap(), b"installer");
    let names = |folder: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&updates.join("0.4.1")).len(), 1);
    assert_eq!(names(&updates), ["0.4.1"]);

    for version in ["../0.4.1", "0.4.1-beta", "", "0..1", "0.4.1/x"] {
        let refused = plugin::save_download(&updates, version, b"x").unwrap_err();
        assert_eq!(
            refused.kind(),
            std::io::ErrorKind::InvalidInput,
            "{version}"
        );
    }
    assert_eq!(names(&updates), ["0.4.1"]);
}

/// An install that failed and could not put the old app back (macOS) says so in its own I/O
/// line, never with the folder it names.
#[test]
fn an_old_app_not_put_back_is_its_own_io_line() {
    let error = PluginError::PreviousAppNotRestored(PathBuf::from("/Applications/Clauseo.app"));
    assert_eq!(
        plugin::failure(&error),
        UpdaterError::Io("the update failed and the old app was not put back".to_owned())
    );
}

/// No failure line carries the plugin's text, which can name a URL or a path.
#[test]
fn no_failure_line_names_a_url_or_a_path() {
    let errors = [
        PluginError::Network("https://example.invalid/x: 404".to_owned()),
        PluginError::Io(std::io::Error::other("/Users/a/Library/x")),
        PluginError::PreviousAppNotRestored(PathBuf::from("/Applications/Clauseo.app")),
        PluginError::TargetsNotFound(vec!["linux-x86_64".to_owned()]),
        PluginError::SignatureUtf8("https://example.invalid".to_owned()),
    ];
    for error in &errors {
        let line = plugin::failure(error).to_string();
        assert!(
            !line.contains("https") && !line.contains("/Users") && !line.contains("/Applications"),
            "{line}"
        );
    }
}
