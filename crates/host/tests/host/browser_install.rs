//! The Chrome for Testing download through the host (S27, decision record 0029): `InstallBrowser`
//! and `CancelBrowserInstall` with fake tools, and what reaches the app subscription. The
//! version and the zip's hash come from a signed browser pins fixture (`pins-131`, signed by the
//! block list's test key A) given as the bundled copy.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::browser::chrome::{ChromeRoots, Platform};
use clauseo_host::browser::install::{Cancel, Failure, InstallTools, record};
use clauseo_host::{AppSubscription, BrowserPins, HostConfig};
use clauseo_protocol::{
    BrowserInstallFailure, BrowserInstallState, CommandOutcome, Count, HostCommand, HostEvent,
    Principal,
};
use clauseo_sandbox::Route;
use futures::StreamExt as _;

use crate::support::{Dir, Running, wait};

#[path = "browser_install_catch_up.rs"]
mod catch_up;
use crate::write;

/// The browser inside the unpacked folder on Apple silicon.
const MAC_BINARY: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";

/// A zip with no entry: only its end of central directory record.
const EMPTY_ZIP: [u8; 22] = [
    0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// The browser pins fixtures (`tests/data/browser-pins/make_fixtures.py`).
const PINS_FIXTURES: &str = include_str!("../data/browser-pins/fixtures.json");

/// Writes the pins fixture `name` into `folder` under the published names.
pub(crate) fn put_pins(folder: &Path, name: &str) {
    let all: serde_json::Value = serde_json::from_str(PINS_FIXTURES).unwrap();
    write::create_dir_all(folder);
    let text = |field: &str| all[name][field].as_str().unwrap().as_bytes().to_vec();
    write::write(&folder.join("browser-pins-v1.json"), &text("body"));
    write::write(
        &folder.join("browser-pins-v1.json.minisig"),
        &text("minisig"),
    );
}

/// The release's pins but the block list's test key A, and `pins-131` (Chrome 131.0.6778.85,
/// its Apple silicon zip [`EMPTY_ZIP`]) written as the bundled copy under `dir`.
pub(crate) fn pins(dir: &Dir) -> BrowserPins {
    let bundled = dir.home().join("bundled-pins");
    put_pins(&bundled, "pins-131");
    let key = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../blocklist/tests/data/key-a.pub"
    ))
    .unwrap();
    BrowserPins {
        keys: vec![key],
        ..BrowserPins::release(Some(bundled))
    }
}

/// Fake tools: the pins give `131.0.6778.85`; the download either finishes at once or, when
/// `block` is set, says it started and runs until released (then finishes) or cancelled.
pub(super) struct Fake {
    block: bool,
    started: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    /// How many downloads began.
    downloads: AtomicUsize,
}

/// A fake's handles: its starts, and the sender that releases a blocked download.
pub(super) struct Handles {
    pub(super) starts: mpsc::Receiver<()>,
    pub(super) release: mpsc::Sender<()>,
}

impl Fake {
    pub(super) fn new(block: bool) -> (Arc<Self>, Handles) {
        let (started, starts) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let fake = Self {
            block,
            started: Mutex::new(started),
            release: Mutex::new(released),
            downloads: AtomicUsize::new(0),
        };
        (Arc::new(fake), Handles { starts, release })
    }
}

impl InstallTools for Fake {
    fn free_bytes(&self, _folder: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }

    fn download(
        &self,
        url: &str,
        _route: &Route,
        zip: &Path,
        max_bytes: u64,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
        // The URL from the template for the pinned version, curl capped at the pin's size.
        assert_eq!(
            url,
            "https://storage.googleapis.com/chrome-for-testing-public/131.0.6778.85/mac-arm64/\
             chrome-mac-arm64.zip"
        );
        assert_eq!(max_bytes, 22);
        self.downloads.fetch_add(1, Ordering::SeqCst);
        write::write(zip, &EMPTY_ZIP);
        progress(22, 22);
        self.started.lock().unwrap().send(()).unwrap();
        if self.block {
            // Polls the cancel as the system tools do, until the test releases it.
            let release = self.release.lock().unwrap();
            while release.recv_timeout(Duration::from_millis(10)).is_err() {
                if cancel.is_cancelled() {
                    return Err(Failure::cancelled());
                }
            }
        }
        Ok(Some(22))
    }

    fn unpack(&self, _zip: &Path, into: &Path, _cancel: &Cancel) -> Result<(), Failure> {
        let binary = MAC_BINARY
            .split('/')
            .fold(into.to_owned(), |path, part| path.join(part));
        write::create_dir_all(binary.parent().unwrap());
        write::write(&binary, b"chrome");
        Ok(())
    }

    fn verify_signature(
        &self,
        _platform: Platform,
        _folder: &Path,
        _cancel: &Cancel,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Apple silicon roots with every folder under `dir`'s data folder, `app/` the app data.
pub(super) fn roots(dir: &Dir) -> ChromeRoots {
    let data = dir.data();
    ChromeRoots {
        platform: Platform::MacArm64,
        root: data.join("root"),
        app_data: data.join("app"),
    }
}

/// Puts a Chrome for Testing of `pins-131`'s version (131.0.6778.85) for `platform` under
/// `app` as the download leaves it, its install record included, and returns its binary.
pub(crate) fn put_chrome_for_testing(app: &Path, platform: Platform) -> PathBuf {
    let version = "131.0.6778.85";
    let folder = app.join("browsers").join(format!("chrome-{version}"));
    let binary = match platform {
        Platform::MacArm64 => MAC_BINARY,
        Platform::MacX64
        | Platform::WindowsX64
        | Platform::WindowsArm64
        | Platform::Linux
        | Platform::Other => "chrome-win64/chrome.exe",
    };
    let binary: PathBuf = binary
        .split('/')
        .fold(folder.clone(), |path, part| path.join(part));
    write::create_dir_all(binary.parent().unwrap());
    write::write(&binary, b"chrome");
    let name = platform.cft_name().unwrap();
    record::write(app, name, version, &"0".repeat(64), &folder).unwrap();
    binary
}

/// Starts a host on `dir` with `tools`, [`roots`] and [`pins`].
fn start(dir: &Dir, tools: Arc<Fake>) -> Running {
    let found = roots(dir);
    let pins = pins(dir);
    dir.start_with(0, move |config: HostConfig| {
        pins.apply(config)
            .with_browser_install_tools(tools)
            .with_chrome_roots(found)
    })
}

/// An app subscription.
pub(super) fn subscribe(host: &Running) -> AppSubscription {
    wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap()
}

/// The download's states on `sub` up to its last (`Done` or `Failed`), skipping other events.
fn states(sub: &mut AppSubscription) -> Vec<BrowserInstallState> {
    wait("the download's states", async {
        let mut states = Vec::new();
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::BrowserInstall { state } = crate::settings::decode(&item) {
                let last = matches!(
                    state,
                    BrowserInstallState::Done | BrowserInstallState::Failed { .. }
                );
                states.push(state);
                if last {
                    return states;
                }
            }
        }
    })
}

/// What `<app>/browsers` holds, by name, sorted.
pub(super) fn browsers(dir: &Dir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.data().join("app").join("browsers"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `InstallBrowser` answers `Done`, the app subscription hears the progress, `Unpacking` and
/// `Done`, and the next session's search finds the browser. A working folder a crash left is
/// gone by then: the start-up sweep removed it before the download began.
#[test]
fn a_download_reaches_the_app_and_the_next_search_finds_it() {
    let dir = Dir::new("browser_install_done", &[]);
    let leftover = dir
        .data()
        .join("app")
        .join("browsers")
        .join(".partial-1-old");
    write::create_dir_all(&leftover.join("chrome"));
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, tools);
    let mut sub = subscribe(&host);
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    assert_eq!(
        states(&mut sub),
        [
            BrowserInstallState::Downloading {
                bytes: Count::from(22u32),
                total: Count::from(22u32),
            },
            BrowserInstallState::Unpacking,
            BrowserInstallState::Done,
        ]
    );
    assert_eq!(browsers(&dir), ["chrome-131.0.6778.85"]);
    let binary: PathBuf = MAC_BINARY.split('/').fold(
        dir.data()
            .join("app")
            .join("browsers")
            .join("chrome-131.0.6778.85"),
        |path, part| path.join(part),
    );
    assert!(binary.is_file());
    // The install record is in place: the launch's check takes the folder.
    let folder = dir.data().join("app/browsers/chrome-131.0.6778.85");
    let app = dir.data().join("app");
    assert_eq!(
        record::verify(&app, "mac-arm64", "131.0.6778.85", &folder),
        Ok(())
    );
    host.shutdown();
}

/// A second `InstallBrowser` while one runs answers `Done` and does nothing: no second
/// download starts, and the first goes on to `Done` as before.
#[test]
fn a_second_install_while_one_runs_is_done_and_starts_nothing() {
    let dir = Dir::new("browser_install_second", &[]);
    let (tools, handles) = Fake::new(true);
    let host = start(&dir, Arc::clone(&tools));
    let mut sub = subscribe(&host);
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    handles.release.send(()).unwrap();
    assert_eq!(states(&mut sub).pop(), Some(BrowserInstallState::Done));
    assert_eq!(tools.downloads.load(Ordering::SeqCst), 1);
    assert_eq!(browsers(&dir), ["chrome-131.0.6778.85"]);
    host.shutdown();
}

/// The first download runs until cancelled, then ends only once the test lets it (the kill
/// steps still running); every later download finishes at once, as `Fake` does.
struct SlowCancel {
    inner: Arc<Fake>,
    started: Mutex<mpsc::Sender<()>>,
    ended: Mutex<mpsc::Receiver<()>>,
}

impl InstallTools for SlowCancel {
    fn free_bytes(&self, folder: &Path) -> std::io::Result<u64> {
        self.inner.free_bytes(folder)
    }

    fn download(
        &self,
        url: &str,
        route: &Route,
        zip: &Path,
        max_bytes: u64,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
        if self.inner.downloads.load(Ordering::SeqCst) > 0 {
            return self
                .inner
                .download(url, route, zip, max_bytes, cancel, progress);
        }
        self.inner.downloads.fetch_add(1, Ordering::SeqCst);
        write::write(zip, &EMPTY_ZIP);
        progress(22, 22);
        self.started.lock().unwrap().send(()).unwrap();
        // Polls the cancel; nothing ever arrives on `idle`.
        let (_keep, idle) = mpsc::channel::<()>();
        while !cancel.is_cancelled() {
            let _waited = idle.recv_timeout(Duration::from_millis(5));
        }
        self.ended.lock().unwrap().recv().unwrap();
        Err(Failure::cancelled())
    }

    fn unpack(&self, zip: &Path, into: &Path, cancel: &Cancel) -> Result<(), Failure> {
        self.inner.unpack(zip, into, cancel)
    }

    fn verify_signature(
        &self,
        platform: Platform,
        folder: &Path,
        cancel: &Cancel,
    ) -> Result<(), String> {
        self.inner.verify_signature(platform, folder, cancel)
    }
}

/// Install, Cancel, Install while the cancelled download is still ending: the second Install
/// is kept, and starts once the first's `Failed { cancelled }` is out; it ends `Done`, two
/// downloads begun (the host review of #337).
#[test]
fn an_install_right_after_a_cancel_starts_once_the_cancelled_one_ends() {
    let dir = Dir::new("browser_install_cancel_again", &[]);
    let (inner, _handles) = Fake::new(false);
    let (started, starts) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    let tools = Arc::new(SlowCancel {
        inner: Arc::clone(&inner),
        started: Mutex::new(started),
        ended: Mutex::new(ended),
    });
    let found = roots(&dir);
    let pins = pins(&dir);
    let host = dir.start_with(0, move |config: HostConfig| {
        pins.apply(config)
            .with_browser_install_tools(tools)
            .with_chrome_roots(found)
    });
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    starts.recv_timeout(Duration::from_secs(30)).unwrap();
    host.ok(HostCommand::CancelBrowserInstall {});
    assert_eq!(
        host.ok(HostCommand::InstallBrowser {}),
        CommandOutcome::Done
    );
    end.send(()).unwrap();
    let first = states(&mut sub).pop().unwrap();
    assert!(
        matches!(
            first,
            BrowserInstallState::Failed {
                code: BrowserInstallFailure::Cancelled,
                ..
            }
        ),
        "{first:?}"
    );
    assert_eq!(states(&mut sub).pop(), Some(BrowserInstallState::Done));
    assert_eq!(inner.downloads.load(Ordering::SeqCst), 2);
    assert_eq!(browsers(&dir), ["chrome-131.0.6778.85"]);
    host.shutdown();
}

/// The cancel ends a running download as `cancelled` with nothing left, and a new one may
/// start after.
#[test]
fn a_cancel_ends_a_running_download() {
    let dir = Dir::new("browser_install_cancel", &[]);
    let (tools, handles) = Fake::new(true);
    let host = start(&dir, tools);
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    assert_eq!(
        host.ok(HostCommand::CancelBrowserInstall {}),
        CommandOutcome::Done
    );
    let last = states(&mut sub).pop().unwrap();
    let BrowserInstallState::Failed { code, .. } = last else {
        panic!("{last:?}");
    };
    assert_eq!(code, BrowserInstallFailure::Cancelled);
    assert_eq!(browsers(&dir), Vec::<String>::new());
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    host.ok(HostCommand::CancelBrowserInstall {});
    assert!(matches!(
        states(&mut sub).pop(),
        Some(BrowserInstallState::Failed { .. })
    ));
    host.shutdown();
}

/// With no signed browser pins that verify (here signed by key A, only key C in the set), the
/// download fails as `list_unavailable` before anything is fetched.
#[test]
fn no_verified_pins_fail_the_download_before_any_fetch() {
    let dir = Dir::new("browser_install_no_pins", &[]);
    let (tools, _handles) = Fake::new(false);
    let found = roots(&dir);
    let pins = BrowserPins {
        keys: vec![
            std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../blocklist/tests/data/key-c.pub"
            ))
            .unwrap(),
        ],
        ..pins(&dir)
    };
    let fake = Arc::clone(&tools);
    let host = dir.start_with(0, move |config: HostConfig| {
        pins.apply(config)
            .with_browser_install_tools(fake)
            .with_chrome_roots(found)
    });
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    let last = states(&mut sub).pop().unwrap();
    let BrowserInstallState::Failed { code, .. } = last else {
        panic!("{last:?}");
    };
    assert_eq!(code, BrowserInstallFailure::ListUnavailable);
    assert_eq!(tools.downloads.load(Ordering::SeqCst), 0);
    host.shutdown();
}

/// With signed pins over 45 days old (`pins-131-46d`), the download fails as
/// `list_unavailable`, saying they expired, before anything is fetched.
#[test]
fn expired_pins_fail_the_download_before_any_fetch() {
    let dir = Dir::new("browser_install_expired", &[]);
    let (tools, _handles) = Fake::new(false);
    let found = roots(&dir);
    let pins = pins(&dir);
    put_pins(pins.bundled.as_deref().unwrap(), "pins-131-46d");
    let fake = Arc::clone(&tools);
    let host = dir.start_with(0, move |config: HostConfig| {
        pins.apply(config)
            .with_browser_install_tools(fake)
            .with_chrome_roots(found)
    });
    let mut sub = subscribe(&host);
    host.ok(HostCommand::InstallBrowser {});
    let last = states(&mut sub).pop().unwrap();
    let BrowserInstallState::Failed { code, message } = last else {
        panic!("{last:?}");
    };
    assert_eq!(code, BrowserInstallFailure::ListUnavailable);
    assert!(message.contains("expired"), "{message}");
    assert_eq!(tools.downloads.load(Ordering::SeqCst), 0);
    host.shutdown();
}

/// `CancelBrowserInstall` with no download running answers `Done` and sends nothing.
#[test]
fn a_cancel_with_nothing_running_does_nothing() {
    let dir = Dir::new("browser_install_idle_cancel", &[]);
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, tools);
    let mut sub = subscribe(&host);
    assert_eq!(
        host.ok(HostCommand::CancelBrowserInstall {}),
        CommandOutcome::Done
    );
    host.settle();
    let heard = catch_up::available(&mut sub);
    assert!(!heard.is_empty(), "the first frames");
    assert_eq!(catch_up::install_states(&heard), []);
    host.shutdown();
}

/// Shutdown during a download cancels it and waits for it: the host closes, and no working
/// folder is left.
#[test]
fn shutdown_cancels_a_running_download() {
    let dir = Dir::new("browser_install_shutdown", &[]);
    let (tools, handles) = Fake::new(true);
    let host = start(&dir, tools);
    host.ok(HostCommand::InstallBrowser {});
    handles
        .starts
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    host.shutdown();
    assert_eq!(browsers(&dir), Vec::<String>::new());
}
