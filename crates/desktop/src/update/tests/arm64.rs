//! The move from x64 to arm64 (`arch.rs`): the gate's whole truth table, the target each check
//! asks for, the fallback to x64 in place when the release has no arm64 installer, and the file
//! binding the download and the install apply for the target that check found. The release is
//! served over HTTPS from 127.0.0.1 (`TestRegistry`'s TLS-inspecting proxy, as in
//! `download_binding.rs`), and the files are named as the real test-v0.0.3 signatures name
//! them (`tests/fixtures/updater/`), signed here with a key made in the test.

use clauseo_agents::testing::TestRegistry;
use clauseo_host::{InstallKind, Offer, ProgressSink, Updater as _, UpdaterError};
use clauseo_settings::Locations;
use serde_json::{Value, json};
use tauri::App;
use tauri::test::MockRuntime;
use tauri_plugin_updater::{Error as PluginError, UpdaterExt as _};

use super::super::arch::{ARM64_KEY, Target, move_to_arm64};
use super::super::file_binding::{expected_update_file, field};
use super::super::plugin::{self, Pending, PluginUpdater, Source};
use super::file_binding::{TestKey, real_trusted_comment};
use super::plugin::{app, for_this_machine};
use crate::admin_refresh::RefreshDecision;
use crate::test_support::{Scratch, block_on};

/// The release's host: never resolved, the proxy answers every `CONNECT`.
const HOST: &str = "updates.clauseo.test";
/// The version of the real signatures, newer than the app's own (`tauri.conf.json`'s 0.0.0).
const VERSION: &str = "0.0.3";
/// The bytes of the entry for this machine's own keys (the x64 installer's, on Windows x64).
const NATIVE_BYTES: &[u8] = b"the native installer's bytes";
/// The bytes of the `windows-aarch64` entry: another file than the native one.
const ARM64_BYTES: &[u8] = b"the arm64 installer's bytes";
/// The tests run on Windows arm64 (the ARM64 runner). There this build's own key is
/// [`ARM64_KEY`] (`for_this_machine`), so every release served here has an arm64 entry: the
/// arm64 check finds it and never falls back, and this build's own installer is the arm64 one. A
/// native arm64 build never asks for the arm64 target (`emulated_on_arm64` is false there).
const WINDOWS_ARM64: bool = cfg!(all(windows, target_arch = "aarch64"));

/// The real test-v0.0.3 trusted comment of `fixture`, its global signature verified with the
/// test key: the file's name and version exactly as the release signs them.
fn real_comment(fixture: &str) -> String {
    let comment = real_trusted_comment(fixture);
    assert_eq!(field(&comment, "version"), Some(VERSION), "{comment}");
    comment
}

/// The real x64 installer's comment.
fn x64_comment() -> String {
    real_comment("real-Clauseo_0.0.3_x64-setup.exe.sig")
}

/// The real arm64 installer's comment.
fn arm64_comment() -> String {
    real_comment("real-Clauseo_0.0.3_arm64-setup.exe.sig")
}

/// A release of [`VERSION`] on [`HOST`], signed with its own key.
struct Release {
    registry: TestRegistry,
    key: TestKey,
}

impl Release {
    fn start() -> Self {
        Self {
            registry: TestRegistry::start_tls_proxy().unwrap(),
            key: TestKey::new(),
        }
    }

    /// An entry serving `bytes` at `path`, signed under `comment`.
    fn entry(&self, path: &str, bytes: &[u8], comment: &str) -> Value {
        self.registry.route(path, bytes.to_vec());
        json!({
            "signature": self.key.sign(bytes, comment),
            "url": format!("https://{HOST}{path}"),
        })
    }

    /// Serves `latest.json`: this machine's own keys (`for_this_machine`) for an entry signed
    /// under `native`, and [`ARM64_KEY`] for one signed under `arm64`, when given.
    fn serve(&self, native: &str, arm64: Option<&str>) {
        let mut manifest = for_this_machine(json!({
            "version": VERSION,
            "notes": "",
            "pub_date": "2026-11-08T09:00:00Z",
            "platforms": {
                "darwin-aarch64-app": self.entry("/native.exe", NATIVE_BYTES, native),
            }
        }));
        if let Some(comment) = arm64 {
            manifest["platforms"][ARM64_KEY] = self.entry("/arm64.exe", ARM64_BYTES, comment);
        }
        let body = serde_json::to_vec(&manifest).unwrap();
        self.registry.route("/latest.json", body);
    }

    fn url() -> url::Url {
        format!("https://{HOST}/latest.json").parse().unwrap()
    }

    fn source(&self) -> Source {
        Source::Endpoint {
            url: Self::url(),
            pubkey: self.key.public(),
        }
    }

    /// The plugin's updater for `target`, as [`Source::updater`] builds it, but through the
    /// proxy and trusting its CA.
    fn updater(
        &self,
        app: &App<MockRuntime>,
        target: Target,
    ) -> Result<tauri_plugin_updater::Updater, PluginError> {
        let proxy = self.registry.proxy_url();
        let ca = self.registry.ca_pem().unwrap();
        let builder = app
            .handle()
            .updater_builder()
            .configure_client(move |builder| {
                plugin::client(builder)
                    .proxy(reqwest::Proxy::all(&proxy).unwrap())
                    .tls_certs_only([reqwest::Certificate::from_pem(ca.as_bytes()).unwrap()])
            })
            .endpoints(vec![Self::url()])?
            .pubkey(self.key.public());
        match target.plugin_target() {
            Some(key) => builder.target(key),
            None => builder,
        }
        .build()
    }

    /// `check_for` against this release, wanting `wanted`.
    fn check(&self, app: &App<MockRuntime>, wanted: Target) -> Option<Offer> {
        let checked =
            plugin::check_for(self.source(), wanted, |_, target| self.updater(app, target));
        tauri::async_runtime::block_on(checked).unwrap()
    }

    /// Whether `path` was requested, inside the tunnel.
    fn fetched(&self, path: &str) -> bool {
        self.registry
            .requests()
            .iter()
            .any(|request| request.tunnelled && request.path == path)
    }
}

/// The target an offer's pending update was found with.
fn target_of(offer: &Offer) -> Target {
    offer.handle.downcast_ref::<Pending>().unwrap().target
}

/// The adapter's download of `offer` into a scratch `updates/`.
fn download(
    app: &App<MockRuntime>,
    release: &Release,
    test: &str,
    offer: Offer,
) -> (Result<clauseo_host::Downloaded, UpdaterError>, Scratch) {
    let scratch = Scratch::new(test, &[]);
    let adapter = PluginUpdater::new(
        app.handle().clone(),
        release.source(),
        InstallKind::CurrentUser,
        scratch.path().join("updates"),
    );
    let (sink, _progress) = ProgressSink::channel(64);
    (block_on(adapter.download(offer, sink)), scratch)
}

/// The refusal of a file signed as another than the expected one.
fn wrong_file() -> Result<(), UpdaterError> {
    Err(UpdaterError::Signature(
        "the signature is for another file than this platform's installer".to_owned(),
    ))
}

/// Every combination: only an emulated x64, installed per user, with an admin copy that is not
/// set up or up to date, moves. Per machine, a read-only `.app`, a package, an admin copy whose
/// refresh is due, one with no installed set-up program to compare, and one that could not be
/// read all stay x64.
#[test]
fn the_gate_moves_only_an_emulated_per_user_install_whose_admin_copy_is_current() {
    let admins = [
        None,
        Some(RefreshDecision::NotSetUp),
        Some(RefreshDecision::NoInstalledCopy),
        Some(RefreshDecision::UpToDate),
        Some(RefreshDecision::Refresh),
    ];
    let mut moved = Vec::new();
    for emulated in [false, true] {
        for kind in InstallKind::ALL {
            for admin in admins {
                if move_to_arm64(emulated, kind, admin) {
                    moved.push((kind, admin));
                }
                let expected = emulated
                    && kind == InstallKind::CurrentUser
                    && matches!(
                        admin,
                        Some(RefreshDecision::NotSetUp | RefreshDecision::UpToDate)
                    );
                assert_eq!(
                    move_to_arm64(emulated, kind, admin),
                    expected,
                    "{emulated} {kind:?} {admin:?}"
                );
            }
        }
    }
    assert_eq!(
        moved,
        [
            (InstallKind::CurrentUser, Some(RefreshDecision::NotSetUp)),
            (InstallKind::CurrentUser, Some(RefreshDecision::UpToDate)),
        ]
    );
}

/// Off Windows, and in any build but x64's, nothing is emulated, so this machine's probe never
/// moves, whatever the install kind.
#[cfg(not(all(windows, target_arch = "x86_64")))]
#[test]
fn off_windows_x64_nothing_is_emulated() {
    use super::super::arch::{emulated_on_arm64, probe};
    assert!(!emulated_on_arm64());
    for kind in InstallKind::ALL {
        assert!(!probe(kind), "{kind:?}");
    }
}

/// The arm64 target looks up `windows-aarch64` alone and binds `arm64-setup.exe` on every OS;
/// the native one sets no target and binds this build's own name.
#[test]
fn each_target_names_its_key_and_its_installer() {
    assert_eq!(Target::Arm64.plugin_target(), Some("windows-aarch64"));
    assert_eq!(Target::Native.plugin_target(), None);
    assert_eq!(
        expected_update_file(VERSION, Target::Arm64).as_deref(),
        field(&arm64_comment(), "file")
    );
    let native = expected_update_file(VERSION, Target::Native);
    if cfg!(all(windows, target_arch = "x86_64")) {
        assert_eq!(native.as_deref(), field(&x64_comment(), "file"));
    }
    // A native arm64 Windows build's own installer is the arm64 one; it never asks for the arm64
    // target (`emulated_on_arm64` is false there), but the two names agree.
    if cfg!(all(windows, target_arch = "aarch64")) {
        assert_eq!(native, expected_update_file(VERSION, Target::Arm64));
    } else {
        assert_ne!(native, expected_update_file(VERSION, Target::Arm64));
    }
}

/// The adapter's check asks for the arm64 installer when its probe moves, and for its own
/// otherwise; the probe is asked with the install kind. The check that follows records the
/// target it found the update with.
#[test]
fn the_check_asks_for_arm64_only_when_the_gate_holds() {
    let release = Release::start();
    release.serve(&x64_comment(), Some(&arm64_comment()));
    let app = app();
    let scratch = Scratch::new("update-arm64-target", &[]);
    let per_user: plugin::Arm64Probe = |kind| kind == InstallKind::CurrentUser;
    let never: plugin::Arm64Probe = |_| false;
    for (kind, probe, expected) in [
        (InstallKind::CurrentUser, per_user, Target::Arm64),
        (InstallKind::CurrentUser, never, Target::Native),
        (InstallKind::PerMachine, per_user, Target::Native),
    ] {
        let adapter = PluginUpdater::new(
            app.handle().clone(),
            release.source(),
            kind,
            scratch.path().join("updates"),
        )
        .with_arm64_probe(probe);
        assert_eq!(adapter.wanted(), expected, "{kind:?}");
        // The adapter's own check builds its updater from the source, which cannot reach the
        // proxy, so `check_for` runs with the same choice through the test's client.
        let offer = release.check(&app, adapter.wanted()).expect("an update");
        assert_eq!(target_of(&offer), expected, "{kind:?}");
        assert_eq!(offer.version, VERSION);
    }
}

/// No `windows-aarch64` in the release: the arm64 check ends in `TargetNotFound`, a second
/// check for this build's own finds the update, and its pending holds the native target. The
/// download and the install then bind this build's installer: a file signed as the other
/// architecture's installer (arm64's, or x64's on an arm64 host, whose own is arm64's) is
/// refused at both, never installed. On Windows arm64 this build's own entry is the arm64 one
/// ([`WINDOWS_ARM64`]): the arm64 check finds it with no fallback, and the x64-signed file is
/// refused all the same.
#[test]
fn without_an_arm64_entry_the_check_falls_back_and_never_binds_arm64() {
    let release = Release::start();
    let other_arch = if cfg!(target_arch = "aarch64") {
        x64_comment()
    } else {
        arm64_comment()
    };
    release.serve(&other_arch, None);
    let app = app();
    let arm64_check =
        tauri::async_runtime::block_on(release.updater(&app, Target::Arm64).unwrap().check());
    let offer = release.check(&app, Target::Arm64).expect("x64 in place");
    if WINDOWS_ARM64 {
        assert!(matches!(arm64_check, Ok(Some(_))));
        assert_eq!(target_of(&offer), Target::Arm64);
        assert_eq!(
            expected_update_file(VERSION, Target::Native),
            expected_update_file(VERSION, Target::Arm64)
        );
    } else {
        assert!(matches!(arm64_check, Err(PluginError::TargetNotFound(_))));
        assert_eq!(target_of(&offer), Target::Native);
    }
    let handle = offer.handle.clone();
    let (downloaded, scratch) = download(&app, &release, "update-arm64-fallback", offer);
    assert!(release.fetched("/native.exe"));
    assert!(!release.fetched("/arm64.exe"));
    let expected = if expected_update_file(VERSION, Target::Native).is_some() {
        wrong_file()
    } else {
        Err(UpdaterError::Signature(
            "this platform installs no update".to_owned(),
        ))
    };
    assert_eq!(downloaded.map(|_| ()), expected);

    // The install rebuilds the update with the pending's target and binds it again.
    let path = scratch.file("Clauseo-setup.exe", NATIVE_BYTES);
    let saved = clauseo_host::Downloaded {
        version: VERSION.to_owned(),
        path,
        handle,
    };
    assert_eq!(plugin::install(app.handle(), &saved), expected);
}

/// The fallback's download of a file signed as this build's own installer is saved (on the
/// platforms that update: Windows x64 names the real x64 installer). On Windows arm64 the arm64
/// check finds this build's own entry with no fallback ([`WINDOWS_ARM64`]), and the file binds
/// as the arm64 installer, which is this build's own.
#[test]
fn the_fallbacks_download_is_bound_to_this_builds_installer() {
    let Some(native) = expected_update_file(VERSION, Target::Native) else {
        return;
    };
    let release = Release::start();
    release.serve(
        &format!("timestamp:1\tfile:{native}\tversion:{VERSION}"),
        None,
    );
    let app = app();
    let offer = release.check(&app, Target::Arm64).expect("x64 in place");
    let found = if WINDOWS_ARM64 {
        Target::Arm64
    } else {
        Target::Native
    };
    assert_eq!(target_of(&offer), found);
    let (downloaded, _scratch) = download(&app, &release, "update-arm64-native", offer);
    assert_eq!(downloaded.unwrap().version, VERSION);
}

/// The release has `windows-aarch64`: the arm64 check finds it, and host.log says the move is
/// offered. Its download is saved only when signed as the arm64 installer, and refused when
/// signed as the x64 one; the install binds the arm64 name too.
#[test]
fn with_an_arm64_entry_the_download_and_install_bind_the_arm64_installer() {
    let scratch = Scratch::folder("update-arm64-log", &[]);
    let locations = Locations::under_home(scratch.path());
    let log = clauseo_host::log_file::install(&locations);
    let app = app();
    for (test, signed_as, accepted) in [
        ("update-arm64-x64", x64_comment(), false),
        ("update-arm64-arm64", arm64_comment(), true),
    ] {
        let release = Release::start();
        release.serve(&x64_comment(), Some(&signed_as));
        let offer = release.check(&app, Target::Arm64).expect("the arm64 build");
        assert_eq!(target_of(&offer), Target::Arm64);
        let handle = offer.handle.clone();
        let (downloaded, scratch) = download(&app, &release, test, offer);
        assert!(release.fetched("/arm64.exe"));
        assert!(!release.fetched("/native.exe"));
        if accepted {
            assert_eq!(downloaded.unwrap().version, VERSION);
            continue;
        }
        assert_eq!(downloaded.map(|_| ()), wrong_file());
        let saved = clauseo_host::Downloaded {
            version: VERSION.to_owned(),
            path: scratch.file("Clauseo-setup.exe", ARM64_BYTES),
            handle,
        };
        assert_eq!(plugin::install(app.handle(), &saved), wrong_file());
    }
    log.flush();
    let text = std::fs::read_to_string(locations.log_dir().join("host.log")).unwrap();
    assert!(
        text.contains("updater: emulated x64 on ARM64; offering the arm64 build 0.0.3"),
        "{text}"
    );
}

/// The folder of last run's download of [`VERSION`].
const CACHED: &str = "updates/0.0.3";

/// Last run's download of `bytes` in [`CACHED`] (made with the scratch), as the host finds it.
fn cache(scratch: &Scratch, bytes: &[u8]) -> std::path::PathBuf {
    scratch.file("updates/0.0.3/Clauseo-setup.exe", bytes)
}

/// `offer` after the adapter's check of last run's file in `scratch`'s `updates/`.
fn cache_checked(release: &Release, scratch: &Scratch, offer: Offer) -> Offer {
    let updates = scratch.path().join("updates");
    super::super::cached::checked(offer, &updates, Some(&release.key.public()))
}

/// Last run saved the x64 installer; this run's gate moves to arm64. The offer asks the host to
/// download again (`refetch`), so the cached file is never made `ready` or installed (the
/// host's side: `update_an_offer_that_refetches_downloads_again_and_never_installs_the_cached_
/// file`). The fresh download replaces it, and the arm64 file then binds and is kept.
#[test]
fn a_cached_x64_download_is_fetched_again_for_an_arm64_offer() {
    let release = Release::start();
    release.serve(&x64_comment(), Some(&arm64_comment()));
    let app = app();
    let scratch = Scratch::new("update-arm64-cache", &[CACHED]);
    let path = cache(&scratch, NATIVE_BYTES);
    let offer = release.check(&app, Target::Arm64).expect("the arm64 build");
    assert_eq!(target_of(&offer), Target::Arm64);
    let offer = cache_checked(&release, &scratch, offer);
    assert!(offer.refetch);

    let adapter = PluginUpdater::new(
        app.handle().clone(),
        release.source(),
        InstallKind::CurrentUser,
        scratch.path().join("updates"),
    );
    let (sink, _progress) = ProgressSink::channel(64);
    let fresh = block_on(adapter.download(offer, sink)).unwrap();
    assert!(release.fetched("/arm64.exe"));
    // The version's folder now holds the fresh file alone.
    assert_eq!(fresh.path.parent(), path.parent());
    assert!(fresh.path == path || !path.exists());
    assert_eq!(std::fs::read(&fresh.path).unwrap(), ARM64_BYTES);
    let again = release.check(&app, Target::Arm64).expect("the arm64 build");
    assert!(!cache_checked(&release, &scratch, again).refetch);
}

/// The reverse: last run saved the arm64 installer, and this run checks for x64 in place (the
/// gate flipped back, or the release has no arm64 entry). Fetched again, never reused. On
/// Windows arm64 there is no x64 offer ([`WINDOWS_ARM64`]): with the arm64 entry the native
/// check finds it, this build's own installer, and the cached arm64 file binds and is reused;
/// without it, the arm64 check finds the x64-signed entry under this build's key, and the cached
/// file is fetched again.
#[test]
fn a_cached_arm64_download_is_fetched_again_for_an_x64_offer() {
    for arm64_entry in [true, false] {
        let release = Release::start();
        let arm64 = arm64_comment();
        release.serve(&x64_comment(), arm64_entry.then_some(arm64.as_str()));
        let app = app();
        let scratch = Scratch::new("update-arm64-cache-back", &[CACHED]);
        cache(&scratch, ARM64_BYTES);
        let wanted = if arm64_entry {
            Target::Native
        } else {
            Target::Arm64
        };
        let offer = release.check(&app, wanted).expect("x64 in place");
        if WINDOWS_ARM64 {
            assert_eq!(target_of(&offer), wanted);
            assert_eq!(
                cache_checked(&release, &scratch, offer).refetch,
                !arm64_entry,
                "{arm64_entry}"
            );
            continue;
        }
        assert_eq!(target_of(&offer), Target::Native);
        assert!(
            cache_checked(&release, &scratch, offer).refetch,
            "{arm64_entry}"
        );
    }
}

/// A cached file that binds to the offer is reused (no `refetch`); none, several files, or an
/// unreadable key leave nothing to reuse or refuse it.
#[test]
fn only_a_cached_file_that_binds_to_the_offer_is_reused() {
    let release = Release::start();
    release.serve(&x64_comment(), Some(&arm64_comment()));
    let app = app();
    let offer = || release.check(&app, Target::Arm64).expect("the arm64 build");
    let empty = Scratch::new("update-arm64-cache-none", &[]);
    assert!(
        !cache_checked(&release, &empty, offer()).refetch,
        "no folder"
    );
    let scratch = Scratch::new("update-arm64-cache-kept", &[CACHED]);
    cache(&scratch, ARM64_BYTES);
    assert!(!cache_checked(&release, &scratch, offer()).refetch);
    let updates = scratch.path().join("updates");
    let unkeyed = super::super::cached::checked(offer(), &updates, None);
    assert!(unkeyed.refetch);
    scratch.file("updates/0.0.3/other", b"x");
    assert!(
        !cache_checked(&release, &scratch, offer()).refetch,
        "two files"
    );
}

/// An arm64 check that fails on the network is that failure: only a missing entry falls back,
/// and no second check is made.
#[test]
fn an_arm64_check_that_fails_on_the_network_does_not_fall_back() {
    let release = Release::start();
    release.serve(&x64_comment(), None);
    release.registry.then(
        "/latest.json",
        clauseo_agents::testing::Behaviour::CutAfter(5),
    );
    let app = app();
    let builds = std::cell::Cell::new(Vec::new());
    let checked = plugin::check_for(release.source(), Target::Arm64, |_, target| {
        let mut seen = builds.take();
        seen.push(target);
        builds.set(seen);
        release.updater(&app, target)
    });
    let result = tauri::async_runtime::block_on(checked);
    assert!(
        matches!(result, Err(UpdaterError::Network(_))),
        "{result:?}"
    );
    assert_eq!(builds.take(), [Target::Arm64]);
    let asked = release
        .registry
        .requests()
        .iter()
        .filter(|request| request.tunnelled && request.path == "/latest.json")
        .count();
    assert_eq!(asked, 1);
}
