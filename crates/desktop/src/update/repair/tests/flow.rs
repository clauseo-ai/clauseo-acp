//! The flow end to end: the release served over HTTPS from 127.0.0.1 (clauseo-agents'
//! `TestRegistry`, as a TLS-inspecting proxy that answers every `CONNECT`, github.com's
//! included, whose test CA the client trusts), signatures made with a key made in the test
//! (#728's `TestKey`, minisign's real `.sig` text), and stub seams over a real file in a
//! scratch folder, which record what ran, in order.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up writes and removes files in its scratch folder; no write module here"
)]

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use clauseo_agents::testing::TestRegistry;

use super::super::super::tests::file_binding::{TestKey, comment};
use super::super::RepairStep;
use super::super::flow::{self, ARGS, Refusal, Seams, Staged};
use super::super::plan::{self, Inputs, Plan};
use super::{VERSION, inputs};
use crate::commands::ReleaseMode;
use crate::test_support::Scratch;

/// This version's x64 installer, as the release signs it.
const X64: &str = "Clauseo_0.4.1_x64-setup.exe";

/// The installer's bytes on the release.
const BYTES: &[u8] = b"the installer's bytes";

/// The stub's file: written and read through one `File`, which a tampering test overwrites by
/// path as it seals.
struct StubStage {
    folder: PathBuf,
    path: PathBuf,
    file: File,
    tamper: bool,
}

impl Staged for StubStage {
    fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        self.file.write_all(chunk)
    }

    fn seal(&mut self) -> io::Result<()> {
        if self.tamper {
            // Another process of the lawyer's, between the download and the check.
            std::fs::write(&self.path, b"something else entirely")?;
        }
        Ok(())
    }

    fn read_back(&mut self) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Where a stub run goes wrong, if anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    /// Nowhere.
    None,
    /// Another process rewrites the file between the download and the check.
    Tamper,
    /// A window won't go.
    WindowStays,
    /// Windows refuses the start.
    LaunchRefused,
    /// The updater, asked again, refuses (an update scheduled, installing or managed).
    UpdateTurnedUp,
    /// The lawyer quit meanwhile.
    Quitting,
}

/// Seams that record each step, launch nothing, and fail where told.
struct Stub {
    root: PathBuf,
    trouble: Trouble,
    events: RefCell<Vec<String>>,
}

impl Stub {
    fn new(root: &Path) -> Self {
        Self {
            root: root.join("attempts"),
            trouble: Trouble::None,
            events: RefCell::new(Vec::new()),
        }
    }

    fn log(&self, event: impl Into<String>) {
        self.events.borrow_mut().push(event.into());
    }

    fn events(&self) -> Vec<String> {
        self.events.borrow().clone()
    }

    /// The attempts' folders left under the root.
    fn folders(&self) -> usize {
        std::fs::read_dir(&self.root).map_or(0, Iterator::count)
    }
}

impl Seams for Stub {
    type Stage = StubStage;

    fn stage(&self, file: &str) -> io::Result<StubStage> {
        self.log(format!("stage {file}"));
        let folder = self.root.join("repair-attempt");
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(file);
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(StubStage {
            folder,
            path,
            file,
            tamper: self.trouble == Trouble::Tamper,
        })
    }

    fn discard(&self, stage: StubStage) {
        self.log("discard");
        drop(stage.file);
        std::fs::remove_dir_all(&stage.folder).unwrap();
    }

    fn announce(&self, step: RepairStep) {
        self.log(format!("announce {step:?}"));
    }

    fn still_eligible(&self) -> bool {
        self.log("recheck");
        self.trouble != Trouble::UpdateTurnedUp
    }

    fn mark(&self) -> io::Result<()> {
        self.log("mark");
        Ok(())
    }

    fn unmark(&self) {
        self.log("unmark");
    }

    fn hand_over(&self) -> bool {
        self.log("hand over");
        self.trouble != Trouble::WindowStays
    }

    fn launch(&self, stage: &StubStage, args: &[&str]) -> io::Result<()> {
        assert!(stage.path.exists(), "launched a file that is not there");
        self.log(format!("launch {}", args.join(" ")));
        if self.trouble == Trouble::LaunchRefused {
            Err(io::Error::other("refused"))
        } else {
            Ok(())
        }
    }

    fn close(&self, stage: StubStage) {
        self.log("close");
        drop(stage);
    }

    fn bring_back(&self) {
        self.log("bring back");
    }

    fn exit(&self) {
        self.log("exit");
    }

    fn quitting(&self) -> bool {
        self.trouble == Trouble::Quitting
    }
}

/// A release whose installer is [`BYTES`] signed as `file` at `version`, and the plan for this
/// version's x64 installer of a test build trusting the key.
fn release(file: &str, version: &str) -> (TestRegistry, Plan) {
    release_in(ReleaseMode::Test, file, version)
}

/// [`release`] for a build of release `mode`.
fn release_in(mode: ReleaseMode, file: &str, version: &str) -> (TestRegistry, Plan) {
    let registry = TestRegistry::start_tls_proxy().unwrap();
    let key = TestKey::new();
    let planned = plan::plan(Inputs {
        mode,
        ..inputs(&key.public())
    })
    .unwrap();
    registry.route(planned.installer.path(), BYTES.to_vec());
    registry.route(
        planned.signature.path(),
        key.sign(BYTES, &comment(file, version)).into_bytes(),
    );
    (registry, planned)
}

/// The updater's client, through the registry's proxy, trusting its CA alone.
fn client(registry: &TestRegistry) -> reqwest::Client {
    drop(super::super::client());
    let proxy = reqwest::Proxy::all(registry.proxy_url()).unwrap();
    let ca = reqwest::Certificate::from_pem(registry.ca_pem().unwrap().as_bytes()).unwrap();
    super::super::super::plugin::client(reqwest::Client::builder())
        .proxy(proxy)
        .tls_certs_only([ca])
        .build()
        .unwrap()
}

/// Runs the flow of `planned` against `registry` over `stub`.
fn run(stub: &Stub, registry: &TestRegistry, planned: &Plan) -> Result<(), Refusal> {
    let client = client(registry);
    tauri::async_runtime::block_on(flow::run(stub, &client, planned))
}

/// What a refusal before the launch ran: the attempt made and discarded, nothing marked,
/// handed over or launched.
fn refused_before_launch(file: &str) -> Vec<String> {
    vec![
        "announce Downloading".to_owned(),
        format!("stage {file}"),
        "discard".to_owned(),
    ]
}

/// The right file at the right version is checked through the held file and launched, after
/// the marker and the windows' hand-over, with exactly `/CLAUSEO_REPAIR`; then the handle
/// closes and the app quits. The folder stays for the installer, which runs from it.
#[test]
fn this_versions_installer_is_checked_then_launched_after_the_windows_go() {
    let scratch = Scratch::new("repair-accepted", &[]);
    let stub = Stub::new(scratch.path());
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Ok(()));
    assert_eq!(ARGS, ["/CLAUSEO_REPAIR"]);
    assert_eq!(
        stub.events(),
        [
            "announce Downloading",
            "stage Clauseo_0.4.1_x64-setup.exe",
            "recheck",
            "announce Launching",
            "mark",
            "hand over",
            "launch /CLAUSEO_REPAIR",
            "close",
            "exit",
        ]
    );
    let saved = scratch
        .path()
        .join("attempts")
        .join("repair-attempt")
        .join(X64);
    assert_eq!(std::fs::read(saved).unwrap(), BYTES);
    let fetched: Vec<_> = registry
        .requests()
        .into_iter()
        .filter(|request| request.tunnelled)
        .map(|request| request.path)
        .collect();
    assert_eq!(
        fetched,
        [
            "/clauseo-ai/clauseo-acp/releases/download/test-v0.4.1/Clauseo_0.4.1_x64-setup.exe.sig",
            "/clauseo-ai/clauseo-acp/releases/download/test-v0.4.1/Clauseo_0.4.1_x64-setup.exe",
        ]
    );
}

/// The same installer signed as another version, by the same key, is refused as
/// `repair.verify_failed`, deleted, and never launched: the path installs this version only.
#[test]
fn an_installer_signed_as_another_version_is_refused_and_deleted() {
    for version in ["0.4.2", "0.4.0", "0.4.1.0"] {
        let scratch = Scratch::new("repair-other-version", &[]);
        let stub = Stub::new(scratch.path());
        let signed_as = format!("Clauseo_{version}_x64-setup.exe");
        let (registry, planned) = release(&signed_as, version);
        assert_eq!(
            run(&stub, &registry, &planned),
            Err(Refusal::VerifyFailed),
            "{version}"
        );
        assert_eq!(stub.events(), refused_before_launch(X64), "{version}");
        assert_eq!(stub.folders(), 0, "{version}");
    }
    // The right name with another version still refuses.
    let scratch = Scratch::new("repair-other-version-same-name", &[]);
    let stub = Stub::new(scratch.path());
    let (registry, planned) = release(X64, "0.4.2");
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::VerifyFailed));
    assert_eq!(stub.folders(), 0);
}

/// Another architecture's installer, and the sandbox tool, signed by the same key at this
/// version, are refused: a repair never moves architecture or runs another file.
#[test]
fn an_installer_signed_as_another_file_is_refused_and_deleted() {
    for other in [
        "Clauseo_0.4.1_arm64-setup.exe",
        "Clauseo_0.4.1_x64-machine-setup.exe",
        "clauseo-sandbox-setup.exe",
    ] {
        let scratch = Scratch::new("repair-other-file", &[]);
        let stub = Stub::new(scratch.path());
        let (registry, planned) = release(other, VERSION);
        assert_eq!(
            run(&stub, &registry, &planned),
            Err(Refusal::VerifyFailed),
            "{other}"
        );
        assert_eq!(stub.events(), refused_before_launch(X64), "{other}");
        assert_eq!(stub.folders(), 0, "{other}");
    }
}

/// A file changed between the download and the check is refused at the check, which reads the
/// held file back, and deleted; nothing is launched.
#[test]
fn a_file_tampered_with_before_the_check_is_refused_and_deleted() {
    let scratch = Scratch::new("repair-tampered", &[]);
    let mut stub = Stub::new(scratch.path());
    stub.trouble = Trouble::Tamper;
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::VerifyFailed));
    assert_eq!(stub.events(), refused_before_launch(X64));
    assert_eq!(stub.folders(), 0);
}

/// A release without this version's installer is `repair.download_failed`, and the attempt
/// is deleted.
#[test]
fn a_missing_installer_is_a_failed_download() {
    let scratch = Scratch::new("repair-missing", &[]);
    let stub = Stub::new(scratch.path());
    let registry = TestRegistry::start_tls_proxy().unwrap();
    let planned = plan::plan(inputs(&TestKey::new().public())).unwrap();
    assert_eq!(
        run(&stub, &registry, &planned),
        Err(Refusal::DownloadFailed)
    );
    assert_eq!(stub.events(), refused_before_launch(X64));
    assert_eq!(stub.folders(), 0);
}

/// A launch that fails after the windows went deletes the attempt and the marker and brings
/// the main window back: `repair.launch_failed`. So does a window that would not go, without
/// a launch.
#[test]
fn a_failed_launch_brings_the_window_back() {
    let scratch = Scratch::new("repair-launch-failed", &[]);
    let mut stub = Stub::new(scratch.path());
    stub.trouble = Trouble::LaunchRefused;
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::LaunchFailed));
    assert_eq!(
        stub.events(),
        [
            "announce Downloading",
            "stage Clauseo_0.4.1_x64-setup.exe",
            "recheck",
            "announce Launching",
            "mark",
            "hand over",
            "launch /CLAUSEO_REPAIR",
            "discard",
            "unmark",
            "bring back",
        ]
    );
    assert_eq!(stub.folders(), 0);

    let scratch = Scratch::new("repair-window-stayed", &[]);
    let mut stub = Stub::new(scratch.path());
    stub.trouble = Trouble::WindowStays;
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::LaunchFailed));
    let events = stub.events();
    assert!(
        !events.iter().any(|event| event.starts_with("launch")),
        "{events:?}"
    );
    assert_eq!(events.last().map(String::as_str), Some("bring back"));
}

/// A release build fetches from the release's `v<version>` tag, end to end over HTTPS, and
/// launches what it checked.
#[test]
fn a_release_build_fetches_from_the_v_tag() {
    let scratch = Scratch::new("repair-release-tag", &[]);
    let stub = Stub::new(scratch.path());
    let (registry, planned) = release_in(ReleaseMode::Release, X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Ok(()));
    let fetched: Vec<_> = registry
        .requests()
        .into_iter()
        .filter(|request| request.tunnelled)
        .map(|request| request.path)
        .collect();
    assert_eq!(
        fetched,
        [
            "/clauseo-ai/clauseo-acp/releases/download/v0.4.1/Clauseo_0.4.1_x64-setup.exe.sig",
            "/clauseo-ai/clauseo-acp/releases/download/v0.4.1/Clauseo_0.4.1_x64-setup.exe",
        ]
    );
    assert!(stub.events().contains(&"launch /CLAUSEO_REPAIR".to_owned()));
}

/// An update scheduled, installing or managed by the time the download is checked (the
/// updater asked again) refuses: the attempt is deleted, nothing is marked, handed over or
/// launched.
#[test]
fn an_update_that_turned_up_during_the_download_refuses() {
    let scratch = Scratch::new("repair-recheck", &[]);
    let mut stub = Stub::new(scratch.path());
    stub.trouble = Trouble::UpdateTurnedUp;
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::NotEligible));
    assert_eq!(
        stub.events(),
        [
            "announce Downloading",
            "stage Clauseo_0.4.1_x64-setup.exe",
            "recheck",
            "discard",
        ]
    );
    assert_eq!(stub.folders(), 0);
}

/// A quit that started meanwhile launches nothing: the attempt and the marker are deleted, and
/// no window is built again.
#[test]
fn a_quit_meanwhile_launches_nothing() {
    let scratch = Scratch::new("repair-quitting", &[]);
    let mut stub = Stub::new(scratch.path());
    stub.trouble = Trouble::Quitting;
    let (registry, planned) = release(X64, VERSION);
    assert_eq!(run(&stub, &registry, &planned), Err(Refusal::LaunchFailed));
    assert_eq!(
        stub.events(),
        [
            "announce Downloading",
            "stage Clauseo_0.4.1_x64-setup.exe",
            "recheck",
            "announce Launching",
            "mark",
            "hand over",
            "discard",
            "unmark",
        ]
    );
    assert_eq!(stub.folders(), 0);
}

/// Each refusal is its own code, never retried silently.
#[test]
fn each_refusal_has_its_code() {
    use clauseo_protocol::ErrorCode;
    for (refusal, code) in [
        (Refusal::NotEligible, ErrorCode::RepairNotEligible),
        (Refusal::DownloadFailed, ErrorCode::RepairDownloadFailed),
        (Refusal::VerifyFailed, ErrorCode::RepairVerifyFailed),
        (Refusal::LaunchFailed, ErrorCode::RepairLaunchFailed),
    ] {
        assert_eq!(refusal.error().code(), code);
    }
}
