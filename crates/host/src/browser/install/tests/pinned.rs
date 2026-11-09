//! The download against the signed pins: none, no pin for this machine, a size or a hash that
//! differs from the pin (refused before anything reads the zip's content), the URL from the
//! template only, and curl capped at the pin's size.

use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::BrowserInstallFailure;

use super::support::{Fake, Length, VERSION, google_url, pin_of, pins_with};
use super::{browsers, job, run_pinned};
use crate::browser::chrome::Platform;
use crate::browser::install::system::download_args;
use crate::browser::install::{Cancel, Failure, manifest};
use crate::browser::pins::{Pin, Pins};
use crate::pipeline::tests::support::TempDir;

/// `run` on Apple silicon with `tools` and `pins` must fail with `code`, leaving `browsers/`
/// empty: the working folder, zip and all, is gone and nothing is installed.
fn fails_pinned(
    test: &str,
    tools: &Fake,
    pins: Option<&Pins>,
    code: BrowserInstallFailure,
) -> Failure {
    let dir = TempDir::new(test);
    let job = job(dir.path(), Platform::MacArm64);
    let (result, _states) = run_pinned(&job, pins, tools, &Cancel::new());
    let failure = result.unwrap_err();
    assert_eq!(failure.code, code, "{failure:?}");
    assert_eq!(browsers(dir.path()), Vec::<String>::new());
    failure
}

/// Two days after the test pins' date.
fn now() -> clauseo_protocol::Timestamp {
    crate::browser::pins::tests::at("2026-11-09T06:00:00Z")
}

#[test]
fn no_pins_is_list_unavailable_and_downloads_nothing() {
    let tools = Fake::working();
    fails_pinned(
        "cft-no-pins",
        &tools,
        None,
        BrowserInstallFailure::ListUnavailable,
    );
    assert_eq!(tools.calls(), Vec::<&str>::new());
}

#[test]
fn pins_without_this_machines_zip_are_unsupported() {
    let tools = Fake::working();
    let pins = Pins::for_tests(
        "2026-11-07T06:00:00Z",
        VERSION,
        &[
            ("chrome", "linux64", pin_of(&tools.zip)),
            // Another download's pin for this machine is not the browser's.
            ("chrome-headless-shell", "mac-arm64", pin_of(&tools.zip)),
        ],
    );
    fails_pinned(
        "cft-no-pin",
        &tools,
        Some(&pins),
        BrowserInstallFailure::Unsupported,
    );
    assert_eq!(tools.calls(), Vec::<&str>::new());
}

#[test]
fn a_hash_mismatch_removes_the_zip_and_never_unpacks() {
    let tools = Fake::working();
    let mut pin = pin_of(&tools.zip);
    pin.sha256 = "0".repeat(64);
    let failure = fails_pinned(
        "cft-hash",
        &tools,
        Some(&pins_with(&pin)),
        BrowserInstallFailure::HashMismatch,
    );
    assert!(failure.message.contains(&pin.sha256), "{}", failure.message);
    // Only the download ran: no unpack, no signature check, nothing read from the zip.
    assert_eq!(tools.calls(), ["download"]);
}

#[test]
fn a_zip_whose_size_is_not_the_pins_is_refused_before_unpacking() {
    // The file and its Content-Length agree; the pin says one byte more.
    let tools = Fake::working();
    let mut pin = pin_of(&tools.zip);
    pin.size += 1;
    fails_pinned(
        "cft-pin-size",
        &tools,
        Some(&pins_with(&pin)),
        BrowserInstallFailure::SizeMismatch,
    );
    assert_eq!(tools.calls(), ["download"]);
    // And a Content-Length equal to the pin, the file not: refused too.
    let tools = Fake {
        length: Length::Wrong(pin.size),
        ..Fake::working()
    };
    fails_pinned(
        "cft-pin-size-length",
        &tools,
        Some(&pins_with(&pin)),
        BrowserInstallFailure::SizeMismatch,
    );
}

#[test]
fn the_url_is_the_template_and_curl_is_capped_at_the_pins_size() {
    let tools = Fake::working();
    let pin = pin_of(&tools.zip);
    let dir = TempDir::new("cft-url");
    let (result, _) = run_pinned(
        &job(dir.path(), Platform::MacArm64),
        Some(&pins_with(&pin)),
        &tools,
        &Cancel::new(),
    );
    result.unwrap();
    assert_eq!(tools.downloads(), [(google_url(VERSION), pin.size)]);
    assert_eq!(
        manifest::zip_url(VERSION, "mac-arm64", "chrome"),
        format!(
            "https://storage.googleapis.com/chrome-for-testing-public/{VERSION}/mac-arm64/\
             chrome-mac-arm64.zip"
        )
    );
    assert_eq!(
        manifest::zip_url("157.0.8103.38", "win64", "chrome-headless-shell"),
        "https://storage.googleapis.com/chrome-for-testing-public/157.0.8103.38/win64/\
         chrome-headless-shell-win64.zip"
    );
    // Windows x64 takes the win64 zip by the same template.
    let tools = Fake::working();
    let dir = TempDir::new("cft-url-win");
    let _result = run_pinned(
        &job(dir.path(), Platform::WindowsX64),
        Some(&pins_with(&pin)),
        &tools,
        &Cancel::new(),
    );
    assert_eq!(
        tools.downloads(),
        [(manifest::zip_url(VERSION, "win64", "chrome"), pin.size)]
    );
    // curl's own cap is the pin's size.
    let args = download_args(
        "https://storage.googleapis.com/z.zip",
        Path::new("/w/headers.txt"),
        Path::new("/w/chrome.zip"),
        126_513_060,
    );
    let at = args.iter().position(|arg| arg == "--max-filesize").unwrap();
    assert_eq!(args.get(at + 1), Some(&OsString::from("126513060")));
    assert_eq!(
        args.iter().filter(|arg| *arg == "--max-filesize").count(),
        1
    );
}

#[test]
fn the_pinned_download_takes_the_chrome_pin_for_the_platform() {
    let pin = Pin {
        sha256: "a".repeat(64),
        size: 5,
    };
    let download = manifest::pinned_download(Some(&pins_with(&pin)), "win64", now()).unwrap();
    assert_eq!(download.version, VERSION);
    assert_eq!(download.sha256, pin.sha256);
    assert_eq!(download.size, 5);
    assert_eq!(download.url, manifest::zip_url(VERSION, "win64", "chrome"));
    assert_eq!(
        manifest::pinned_download(None, "win64", now())
            .unwrap_err()
            .code,
        BrowserInstallFailure::ListUnavailable
    );
}

#[test]
fn pins_over_45_days_old_are_list_unavailable_and_download_nothing() {
    let tools = Fake::working();
    let dir = TempDir::new("cft-expired");
    let mut late = job(dir.path(), Platform::MacArm64);
    // The test pins are dated 2026-11-07T06:00:00Z: 45 days and a second later.
    late.now = crate::browser::pins::tests::at("2026-12-22T06:00:01Z");
    let (result, _) = run_pinned(&late, Some(&tools.pins()), &tools, &Cancel::new());
    let failure = result.unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::ListUnavailable);
    assert!(failure.message.contains("expired"), "{}", failure.message);
    assert_eq!(tools.calls(), Vec::<&str>::new());
    assert_eq!(browsers(dir.path()), Vec::<String>::new());
    // 45 days exactly: still taken.
    let tools = Fake::working();
    let dir = TempDir::new("cft-45-days");
    let mut edge = job(dir.path(), Platform::MacArm64);
    edge.now = crate::browser::pins::tests::at("2026-12-22T06:00:00Z");
    let (result, _) = run_pinned(&edge, Some(&tools.pins()), &tools, &Cancel::new());
    result.unwrap();
}
