//! The Chrome for Testing download through the host (S27, decision record 0029): `InstallBrowser`
//! and `CancelBrowserInstall` with fake tools, and what reaches the app subscription.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::browser::chrome::{self, ChromeRoots, Platform};
use clauseo_host::browser::install::{Cancel, Failure, InstallTools};
use clauseo_host::{AppSubscription, HostConfig};
use clauseo_protocol::{
    BrowserInstallFailure, BrowserInstallState, CommandOutcome, Count, HostCommand, HostEvent,
    Principal,
};
use futures::StreamExt as _;

use crate::support::{Dir, Running, wait};
use crate::write;

/// The browser inside the unpacked folder on Apple silicon.
const MAC_BINARY: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";

/// A zip with no entry: only its end of central directory record.
const EMPTY_ZIP: [u8; 22] = [
    0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Fake tools: Stable is `131.0.6778.85`; the download either finishes at once or, when
/// `block` is set, says it started and runs until released (then finishes) or cancelled.
struct Fake {
    block: bool,
    started: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    /// How many downloads began.
    downloads: AtomicUsize,
}

/// A fake's handles: its starts, and the sender that releases a blocked download.
struct Handles {
    starts: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl Fake {
    fn new(block: bool) -> (Arc<Self>, Handles) {
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

    fn fetch_versions(
        &self,
        _url: &str,
        _work: &Path,
        _cancel: &Cancel,
    ) -> Result<Vec<u8>, Failure> {
        let version = "131.0.6778.85";
        let url = format!(
            "https://storage.googleapis.com/chrome-for-testing-public/{version}/mac-arm64/\
             chrome-mac-arm64.zip"
        );
        let list = serde_json::json!({"channels": {"Stable": {
            "version": version,
            "downloads": {"chrome": [{"platform": "mac-arm64", "url": url}]},
        }}});
        Ok(list.to_string().into_bytes())
    }

    fn download(
        &self,
        _url: &str,
        zip: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
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
fn roots(dir: &Dir) -> ChromeRoots {
    let data = dir.data();
    ChromeRoots {
        platform: Platform::MacArm64,
        root: data.join("root"),
        home: None,
        program_files: None,
        program_files_x86: None,
        local_app_data: None,
        app_data: data.join("app"),
    }
}

/// Starts a host on `dir` with `tools` and [`roots`].
fn start(dir: &Dir, tools: Arc<Fake>) -> Running {
    let found = roots(dir);
    dir.start_with(0, move |config: HostConfig| {
        config
            .with_browser_install_tools(tools)
            .with_chrome_roots(found)
    })
}

/// An app subscription.
fn subscribe(host: &Running) -> AppSubscription {
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
fn browsers(dir: &Dir) -> Vec<String> {
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
    assert_eq!(chrome::find(&roots(&dir)), Some(binary));
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

/// `CancelBrowserInstall` with no download running answers `Done` and sends nothing.
#[test]
fn a_cancel_with_nothing_running_does_nothing() {
    let dir = Dir::new("browser_install_idle_cancel", &[]);
    let (tools, _handles) = Fake::new(false);
    let host = start(&dir, tools);
    assert_eq!(
        host.ok(HostCommand::CancelBrowserInstall {}),
        CommandOutcome::Done
    );
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
