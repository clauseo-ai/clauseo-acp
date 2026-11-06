//! The updater in desktop: which updater a build picks, the test key kept out of release builds,
//! the install kind's decisions, the install's order, the plugin adapter, and S20's `latest.json`
//! fixtures through the plugin's own types (done-when 2 of R23).
//!
//! The fixtures (`fixtures/`) are S20's static format (`.github/scripts/latest-json.mjs`), version
//! 0.4.1, signed with a fixture key made for these tests alone (`tauri signer generate`; only
//! its public half, `fixture.pub`, is committed) over `payload.bin`, with `--app-version 0.4.1`:
//!
//! - `latest.json`: every platform key S20 writes, each with the payload's signature (passes
//!   `manifestProblems`);
//! - `latest-bad-signature.json`: the same, but each signature is of other bytes;
//! - `latest-mac-only.json`: `darwin-aarch64-app` alone (not a whole S20 manifest on purpose).
//!
//! No test reaches the network: `restore_update` reads a fixture without a request, and the one
//! check made goes to a closed port on 127.0.0.1.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

#[cfg(feature = "e2e")]
use clauseo_host::Updater as _;
use clauseo_host::{Downloaded, HostExit, InstallKind, Opaque, UpdaterError};
use clauseo_settings::Locations;
use serde_json::Value;
use tauri::test::MockRuntime;

use super::choice::{self, Build, Choice, E2eEnv, Flavour, TEST_KEY, TRUSTS_TEST_KEY};
use super::install::{self, After, Steps, Watch};
use super::install_kind::{bundle_folder, mac_kind, windows_kind};
#[cfg(feature = "e2e")]
use super::plugin::Pending;
use crate::test_support::Scratch;
#[cfg(feature = "e2e")]
use crate::test_support::block_on;

mod plugin;
mod release_key;
mod test_key;
mod watchdog;

/// The crate's folder.
pub(super) fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The committed test key's public half (`release/updater-test.pub`).
pub(super) fn test_key_file() -> String {
    std::fs::read_to_string(crate_dir().join("release/updater-test.pub"))
        .unwrap()
        .trim()
        .to_owned()
}

/// A release build: not Linux, not debug, not `e2e`, no test key.
const RELEASE: Build = Build {
    flavour: Flavour::Release,
    trusts_test_key: false,
    endpoints: true,
};

/// The `e2e` build, which trusts the test key.
const E2E: Build = Build {
    flavour: Flavour::E2e,
    trusts_test_key: true,
    ..RELEASE
};

/// The `E2E_*` variables as given.
fn env(fake: Option<&str>, endpoint: Option<&str>) -> E2eEnv {
    E2eEnv {
        fake: fake.map(str::to_owned),
        endpoint: endpoint.map(str::to_owned),
    }
}

#[test]
fn a_build_with_no_endpoint_gives_the_host_no_updater() {
    let none = E2eEnv::default();
    assert_eq!(choice::choose(RELEASE, none.clone()), Choice::Config);
    let no_endpoint = Build {
        endpoints: false,
        ..RELEASE
    };
    assert_eq!(choice::choose(no_endpoint, none.clone()), Choice::None);
    let debug = Build {
        flavour: Flavour::Debug,
        ..RELEASE
    };
    assert_eq!(choice::choose(debug, none), Choice::None);

    // Since R23 PR 6 the checked config names the release endpoint, so a release build takes the
    // plugin over it, and a debug build still none.
    let context = crate::context::<MockRuntime>();
    assert!(choice::has_endpoints(context.config()));
}

#[test]
fn the_e2e_build_takes_its_updater_only_from_its_variables() {
    let url = "https://github.com/clauseo-ai/clauseo-acp/releases/download/test-v0.0.2/latest.json";
    assert_eq!(
        choice::choose(E2E, env(None, Some(url))),
        Choice::TestRelease(url.parse().unwrap())
    );
    // The fake first, so a spec never reaches an endpoint.
    assert_eq!(
        choice::choose(E2E, env(Some(" 0.0.2 "), Some(url))),
        Choice::Fake("0.0.2".to_owned())
    );
    // Neither, or no URL: none, even with the config's endpoints.
    assert_eq!(choice::choose(E2E, E2eEnv::default()), Choice::None);
    assert_eq!(
        choice::choose(E2E, env(Some(""), Some("not a url"))),
        Choice::None
    );
    // An endpoint without the test key trusted is never taken.
    let untrusting = Build {
        trusts_test_key: false,
        ..E2E
    };
    assert_eq!(
        choice::choose(untrusting, env(None, Some(url))),
        Choice::None
    );
}

#[test]
fn a_release_build_never_takes_the_test_release_or_the_fake() {
    let url = "https://example.invalid/latest.json";
    for build in [
        RELEASE,
        Build {
            trusts_test_key: true,
            ..RELEASE
        },
    ] {
        assert_eq!(
            choice::choose(build, env(Some("9.9.9"), Some(url))),
            Choice::Config
        );
    }
}

/// (b) of "release builds reject the test key": outside the `e2e` build the key is not compiled
/// in, nothing trusts it, and no `E2E_*` variable is read.
#[cfg(not(feature = "e2e"))]
#[test]
fn a_build_without_e2e_never_trusts_the_test_key() {
    // black_box: read as the code reads it, not folded into a constant assert.
    assert!(!std::hint::black_box(TRUSTS_TEST_KEY));
    assert_eq!(TEST_KEY, None);
    assert_eq!(choice::e2e_env(), E2eEnv::default());
}

/// The `e2e` build trusts the committed test key, exactly.
#[cfg(feature = "e2e")]
#[test]
fn the_e2e_build_trusts_the_committed_test_key() {
    // black_box: read as the code reads it, not folded into a constant assert.
    assert!(std::hint::black_box(TRUSTS_TEST_KEY));
    assert_eq!(TEST_KEY.map(str::trim), Some(test_key_file().as_str()));
    // Read, whatever the variables say here.
    let _read = choice::e2e_env();
}

/// (a) of "release builds reject the test key": neither `tauri.conf.json` nor any release
/// overlay (`release/*.conf.json`) carries the test key as the updater's pubkey.
#[test]
fn no_release_config_trusts_the_test_key() {
    let test_key = test_key_file();
    assert!(test_key.len() > 40, "release/updater-test.pub looks empty");
    let mut configs = vec![crate_dir().join("tauri.conf.json")];
    for entry in std::fs::read_dir(crate_dir().join("release")).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with(".conf.json") {
            configs.push(path);
        }
    }
    assert!(configs.len() > 1, "no release overlay was read");
    for config in configs {
        let json: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        let pubkey = json
            .pointer("/plugins/updater/pubkey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        assert!(
            pubkey.is_empty() || !(pubkey == test_key || pubkey.contains(&test_key)),
            "{} trusts the test key",
            config.display()
        );
    }
}

#[test]
fn the_config_requires_a_signed_version_and_no_downgrade_and_installs_passively() {
    let conf: Value =
        serde_json::from_slice(&std::fs::read(crate_dir().join("tauri.conf.json")).unwrap())
            .unwrap();
    let config: tauri_plugin_updater::Config =
        serde_json::from_value(conf["plugins"]["updater"].clone()).unwrap();
    assert!(config.require_signed_version);
    assert!(!config.allow_downgrades);
    let windows = config.windows.unwrap();
    assert_eq!(windows.install_mode.to_string(), "passive");
    assert_eq!(windows.install_mode.nsis_args(), ["/P"]);
}

#[test]
fn windows_is_per_machine_under_program_files_or_with_the_machine_s_uninstall_key() {
    let denied = || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(windows_kind(true, Ok(false)), InstallKind::PerMachine);
    assert_eq!(windows_kind(false, Ok(true)), InstallKind::PerMachine);
    assert_eq!(windows_kind(false, Ok(false)), InstallKind::CurrentUser);
    // A key that could not be read counts as there: no self-update of a machine's install.
    assert_eq!(windows_kind(false, denied()), InstallKind::PerMachine);
}

#[test]
fn a_mac_app_the_user_cannot_move_out_of_its_folder_is_a_read_only_location() {
    let exe = Path::new("/Applications/Clauseo.app/Contents/MacOS/Clauseo");
    let folder = Path::new("/Applications");
    let app = Path::new("/Applications/Clauseo.app");
    assert_eq!(bundle_folder(exe), Some(folder));
    let asked = std::cell::RefCell::new(Vec::new());
    // Writable: the paths in `yes`.
    let writable = |yes: &'static [&'static str]| {
        let asked = &asked;
        move |path: &Path| {
            asked.borrow_mut().push(path.to_path_buf());
            yes.iter().any(|yes| Path::new(yes) == path)
        }
    };
    let both = &["/Applications", "/Applications/Clauseo.app"][..];
    assert_eq!(
        mac_kind(Some(exe), writable(both)),
        InstallKind::CurrentUser
    );
    let mut asked_now: Vec<PathBuf> = asked.borrow_mut().drain(..).collect();
    asked_now.sort();
    assert_eq!(asked_now, [folder.to_path_buf(), app.to_path_buf()]);
    // The plugin moves the old bundle out of its folder: that needs both writable. A root-owned
    // `.app` (an MDM install) in an admin-writable `/Applications` would ask for an
    // administrator.
    for yes in [
        &["/Applications"][..],
        &["/Applications/Clauseo.app"][..],
        &[][..],
    ] {
        assert_eq!(
            mac_kind(Some(exe), writable(yes)),
            InstallKind::ReadOnlyLocation,
            "{yes:?}"
        );
    }
    // Not inside an `.app`: nothing the plugin could swap.
    for exe in [
        "/Users/a/clauseo/target/release/clauseo-desktop",
        "/Applications/Clauseo/Contents/MacOS/Clauseo",
        "/Applications/Clauseo.app/MacOS/Clauseo",
    ] {
        assert_eq!(bundle_folder(Path::new(exe)), None, "{exe}");
        assert_eq!(
            mac_kind(Some(Path::new(exe)), |_: &Path| true),
            InstallKind::ReadOnlyLocation
        );
    }
    assert_eq!(
        mac_kind(None, |_: &Path| true),
        InstallKind::ReadOnlyLocation
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_is_always_a_package() {
    assert_eq!(super::install_kind::detect(), InstallKind::Package);
}

/// The install's steps, recorded.
pub(super) struct Recorded {
    installable: bool,
    install: Result<(), UpdaterError>,
    pub(super) steps: Vec<&'static str>,
}

impl Recorded {
    pub(super) fn new(installable: bool, install: Result<(), UpdaterError>) -> Self {
        Self {
            installable,
            install,
            steps: Vec::new(),
        }
    }
}

impl Steps for Recorded {
    fn installable(&self, _downloaded: &Downloaded) -> bool {
        self.installable
    }
    fn save_window_state(&mut self) {
        self.steps.push("window state");
    }
    fn crash_clean_exit(&mut self) {
        install::crash_clean_exit();
        self.steps.push("crash clean exit");
    }
    fn flush_log(&mut self) {
        self.steps.push("flush");
    }
    fn start_install(&mut self, downloaded: Downloaded) -> Receiver<Result<(), UpdaterError>> {
        assert_eq!(downloaded.version, "0.4.1");
        self.steps.push("install");
        let (answer, answered) = std::sync::mpsc::channel();
        answer.send(self.install.clone()).unwrap();
        answered
    }
    fn app_in_place(&self) -> bool {
        true
    }
}

/// `after_host` with the agents stopped and the real watch.
fn after_host(ended: HostExit, steps: &mut Recorded) -> After {
    install::after_host(ended, true, steps, Watch::INSTALL)
}

/// A download, as the host hands it back.
pub(super) fn downloaded() -> Downloaded {
    Downloaded {
        version: "0.4.1".to_owned(),
        path: PathBuf::from("updates/0.4.1/Clauseo-setup.exe"),
        handle: Opaque::new(()),
    }
}

#[test]
fn an_install_saves_the_window_state_first_and_restarts_after() {
    let mut steps = Recorded::new(true, Ok(()));
    let after = after_host(HostExit::Install(downloaded()), &mut steps);
    assert_eq!(after, After::Restart);
    assert_eq!(
        steps.steps,
        [
            "window state",
            "crash clean exit",
            "flush",
            "install",
            "flush"
        ]
    );
}

#[test]
fn a_failed_install_restarts_as_the_old_version() {
    let failed = Err(UpdaterError::Signature("refused".to_owned()));
    let mut steps = Recorded::new(true, failed);
    let after = after_host(HostExit::Install(downloaded()), &mut steps);
    assert_eq!(after, After::Restart);
    assert_eq!(
        steps.steps,
        [
            "window state",
            "crash clean exit",
            "flush",
            "install",
            "flush"
        ]
    );
}

/// The host log (0047) keeps that an install ran: its version, on the desktop's lifecycle target,
/// and how it went (versions and kinds only, no path). `install` sets the process's global
/// subscriber; nextest runs each test in its own process.
#[test]
fn the_host_log_keeps_that_an_install_ran_and_how_it_went() {
    let scratch = Scratch::folder("update-log", &[]);
    let locations = Locations::under_home(scratch.path());
    let log = clauseo_host::log_file::install(&locations);
    let mut steps = Recorded::new(true, Ok(()));
    after_host(HostExit::Install(downloaded()), &mut steps);
    let failed = Err(UpdaterError::Io(
        "the update failed and the old app was not put back".to_owned(),
    ));
    let mut steps = Recorded::new(true, failed);
    after_host(HostExit::Install(downloaded()), &mut steps);
    log.flush();
    let text = std::fs::read_to_string(locations.log_dir().join("host.log")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let line = |words: &str| {
        lines
            .iter()
            .find(|line| line.contains(words))
            .unwrap_or_else(|| panic!("no line with {words:?} in {text}"))
            .to_owned()
    };
    let installing = line("installing the update");
    assert!(
        installing.contains("clauseo_desktop::lifecycle") && installing.contains("0.4.1"),
        "{installing}"
    );
    let installed = line("the update is installed");
    assert!(
        installed.contains("clauseo_desktop::lifecycle") && installed.contains("0.4.1"),
        "{installed}"
    );
    let failed = line("the update did not install");
    assert!(
        failed.contains("old app was not put back") && failed.contains("io"),
        "{failed}"
    );
    assert!(
        !text.contains(&scratch.path().display().to_string()),
        "no path: {text}"
    );
}

#[test]
fn a_plain_stop_or_a_download_no_plugin_made_installs_nothing() {
    let mut steps = Recorded::new(true, Ok(()));
    assert_eq!(after_host(HostExit::Stopped, &mut steps), After::Exit);
    let mut fake = Recorded::new(false, Ok(()));
    assert_eq!(
        after_host(HostExit::Install(downloaded()), &mut fake),
        After::Exit
    );
    assert!(steps.steps.is_empty() && fake.steps.is_empty());
}

/// The `e2e` build's fake: managed by nobody, offers its version at once, and its download is
/// one no plugin made, so nothing installs it.
#[cfg(feature = "e2e")]
#[test]
fn the_fake_offers_its_version_at_once_and_its_download_installs_nothing() {
    let fake = super::fake::FakeUpdater::new("0.0.2".to_owned());
    assert_eq!(fake.install_kind(), InstallKind::CurrentUser);
    let offer = block_on(fake.check()).unwrap().unwrap();
    assert_eq!(offer.version, "0.0.2");
    let (sink, mut progress) = clauseo_host::ProgressSink::channel(4);
    let downloaded = block_on(fake.download(offer, sink)).unwrap();
    assert_eq!(downloaded.version, "0.0.2");
    assert!(progress.try_recv().is_ok());
    assert!(downloaded.handle.downcast_ref::<Pending>().is_none());
}
