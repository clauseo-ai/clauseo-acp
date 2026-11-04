//! The download engine with fake tools, and its checks one by one.

mod review;
mod support;

use std::path::{Path, PathBuf};

use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, Count};

use super::system::{content_length, curl_exit};
use super::{Cancel, Failure, Job, archive, check_signature, manifest, run};
use crate::browser::chrome::{self, ChromeRoots, Platform};
use crate::pipeline::tests::support::TempDir;
use crate::write;
use support::{Fake, Length, MAC_BINARY, Made, VERSION, ZipEntry, file, google_url, versions, zip};

/// A job for `platform` with the app data folder under `dir`.
fn job(dir: &Path, platform: Platform) -> Job {
    Job {
        platform,
        app_data: dir.join("app"),
        random: "r".to_owned(),
    }
}

/// Runs `job` with `tools`, returning its result and the states it reported.
fn run_with(
    job: &Job,
    tools: &Fake,
    cancel: &Cancel,
) -> (Result<PathBuf, Failure>, Vec<BrowserInstallState>) {
    let mut states = Vec::new();
    let result = run(job, tools, cancel, &mut |state| states.push(state));
    (result, states)
}

/// What `browsers/` holds, by name, sorted.
fn browsers(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("app").join("browsers"))
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// `run` with `tools` on Apple silicon must fail with `code`, leaving only `browsers/` empty.
fn fails_with(test: &str, tools: &Fake, code: BrowserInstallFailure) -> Failure {
    let dir = TempDir::new(test);
    let (result, _states) = run_with(&job(dir.path(), Platform::MacArm64), tools, &Cancel::new());
    let failure = result.unwrap_err();
    assert_eq!(failure.code, code, "{failure:?}");
    assert_eq!(browsers(dir.path()), Vec::<String>::new());
    failure
}

/// A download reports its progress, then `Unpacking`; the folder it makes is the one
/// `chrome::find` finds at the next session, and no working folder is left.
#[test]
fn a_download_reports_progress_then_unpacking_and_find_finds_it() {
    let dir = TempDir::new("cft-done");
    let tools = Fake::working();
    let (result, states) = run_with(&job(dir.path(), Platform::MacArm64), &tools, &Cancel::new());
    let folder = result.unwrap();
    let total = u64::try_from(tools.zip.len()).unwrap();
    assert_eq!(
        states,
        [
            BrowserInstallState::Downloading {
                bytes: Count::clamped(total / 2),
                total: Count::clamped(total),
            },
            BrowserInstallState::Downloading {
                bytes: Count::clamped(total),
                total: Count::clamped(total),
            },
            BrowserInstallState::Unpacking,
        ]
    );
    assert_eq!(
        tools.calls(),
        ["fetch_versions", "download", "unpack", "verify_signature"]
    );
    let browsers_dir = dir.path().join("app").join("browsers");
    assert_eq!(folder, browsers_dir.join(format!("chrome-{VERSION}")));
    assert_eq!(browsers(dir.path()), [format!("chrome-{VERSION}")]);
    let roots = ChromeRoots {
        platform: Platform::MacArm64,
        root: dir.path().join("root"),
        home: None,
        program_files: None,
        program_files_x86: None,
        local_app_data: None,
        app_data: dir.path().join("app"),
    };
    let binary = MAC_BINARY
        .split('/')
        .fold(folder, |path, part| path.join(part));
    assert_eq!(chrome::find(&roots), Some(binary));
}

/// A folder of the same version already there (its binary gone, so `find` passed over it) is
/// replaced.
#[test]
fn an_old_folder_of_the_same_version_is_replaced() {
    let dir = TempDir::new("cft-replace");
    let old = dir
        .path()
        .join("app")
        .join("browsers")
        .join(format!("chrome-{VERSION}"));
    write::create_dir_all(&old).unwrap();
    write::write_file(&old.join("stale"), b"").unwrap();
    let (result, _states) = run_with(
        &job(dir.path(), Platform::MacArm64),
        &Fake::working(),
        &Cancel::new(),
    );
    let folder = result.unwrap();
    assert!(!folder.join("stale").exists());
    assert!(folder.join("chrome-mac-arm64").is_dir());
}

/// A cancel during the download ends it as `cancelled`, with nothing left in `browsers/`.
#[test]
fn a_cancel_during_the_download_leaves_no_folder() {
    let tools = Fake {
        cancel_in_download: true,
        ..Fake::working()
    };
    fails_with("cft-cancel", &tools, BrowserInstallFailure::Cancelled);
    assert_eq!(tools.calls(), ["fetch_versions", "download"]);
}

/// A cancel set before the download starts stops it before the version list is read.
#[test]
fn a_cancel_before_the_start_downloads_nothing() {
    let dir = TempDir::new("cft-cancel-first");
    let cancel = Cancel::new();
    cancel.cancel();
    let tools = Fake::working();
    let (result, states) = run_with(&job(dir.path(), Platform::MacArm64), &tools, &cancel);
    assert_eq!(result.unwrap_err().code, BrowserInstallFailure::Cancelled);
    assert_eq!(states, []);
    assert_eq!(tools.calls(), Vec::<&str>::new());
}

/// A zip whose size differs from its `Content-Length`, or a response with none, is refused.
#[test]
fn a_size_mismatch_or_a_missing_length_is_refused() {
    let longer = Fake {
        length: Length::Wrong(1),
        ..Fake::working()
    };
    fails_with("cft-size", &longer, BrowserInstallFailure::SizeMismatch);
    let none = Fake {
        length: Length::Missing,
        ..Fake::working()
    };
    fails_with("cft-no-length", &none, BrowserInstallFailure::SizeMismatch);
    assert_eq!(none.calls(), ["fetch_versions", "download"]);
}

/// A version list naming another host's URL, or a version that is not dotted digits, is
/// refused before anything downloads.
#[test]
fn a_refused_url_or_version_downloads_nothing() {
    let other_host = Fake {
        versions: versions(VERSION, "https://example.com/chrome-mac-arm64.zip"),
        ..Fake::working()
    };
    fails_with("cft-host", &other_host, BrowserInstallFailure::HostRefused);
    assert_eq!(other_host.calls(), ["fetch_versions"]);
    let climbing = Fake {
        versions: versions("../131", &google_url("../131")),
        ..Fake::working()
    };
    fails_with("cft-version", &climbing, BrowserInstallFailure::HostRefused);
    let not_json = Fake {
        versions: b"<html>".to_vec(),
        ..Fake::working()
    };
    fails_with("cft-json", &not_json, BrowserInstallFailure::HostRefused);
}

/// Each way a zip URL can differ from Google's is refused, and says which.
#[test]
fn zip_urls_must_be_exactly_googles() {
    let refused = |url: &str| {
        let failure = manifest::check_zip_url(url, VERSION, "mac-arm64").unwrap_err();
        assert_eq!(failure.code, BrowserInstallFailure::HostRefused);
        failure.message
    };
    let good = google_url(VERSION);
    manifest::check_zip_url(&good, VERSION, "mac-arm64").unwrap();
    assert!(refused(&good.replace("https://", "http://")).contains("not https"));
    assert!(refused(&good.replace("storage.googleapis.com", "evil.example")).contains("host"));
    assert!(refused(&good.replace(".com/", ".com:8443/")).contains("host"));
    assert!(refused(&good.replace("https://", "https://user@")).contains("host"));
    assert!(refused(&good.replace(VERSION, "131.0.1.1")).contains("version"));
    assert!(refused(&format!("{good}?x=1")).contains("path"));
    assert!(refused(&good.replace("mac-arm64.zip", "mac-x64.zip")).contains("path"));
    assert!(refused(&good.replace("/mac-arm64/", "/mac-arm64/../")).contains("path"));
}

/// Versions are dotted digits of bounded length; the version list's own URL is Google's.
#[test]
fn versions_and_the_list_url_are_checked() {
    for version in ["131.0.6778.85", "1"] {
        manifest::check_version(version).unwrap();
    }
    for version in ["", "131.0.x", "../1", "1/2", "131.0 .1", &"1.".repeat(40)] {
        assert!(manifest::check_version(version).is_err(), "{version:?}");
    }
    manifest::check_versions_url(super::VERSIONS_URL).unwrap();
    assert!(manifest::check_versions_url("http://googlechromelabs.github.io/x.json").is_err());
    assert!(manifest::check_versions_url("https://googlechromelabs.github.io.evil/x").is_err());
}

/// A zip entry that climbs out with `..` is refused before the unpack tool runs, and nothing
/// is left.
#[test]
fn a_climbing_zip_entry_is_refused_before_unpacking() {
    let tools = Fake {
        zip: zip(&[
            file(MAC_BINARY, b"x"),
            file("chrome-mac-arm64/../../evil", b"x"),
        ]),
        ..Fake::working()
    };
    let failure = fails_with("cft-dotdot", &tools, BrowserInstallFailure::Unpack);
    assert!(failure.message.contains(".."), "{}", failure.message);
    assert_eq!(tools.calls(), ["fetch_versions", "download"]);
}

/// Every name the zip check refuses, and one it takes.
#[test]
fn zip_entry_names_are_checked() {
    let checks = |entries: &[ZipEntry<'_>]| {
        let dir = TempDir::new("cft-names");
        let path = dir.path().join("t.zip");
        write::write_file(&path, &zip(entries)).unwrap();
        archive::check_zip(&path)
    };
    checks(&[file("a/b.txt", b"1"), file("a/c.txt", b"")]).unwrap();
    for name in [
        "/etc/x",
        "\\x",
        "C:x",
        "a/../../x",
        "a\\..\\x",
        "a/\u{7}",
        "",
        "r\u{e9}sum\u{e9}",
    ] {
        assert!(checks(&[file(name, b"")]).is_err(), "{name:?}");
    }
    assert!(checks(&[file("a/B", b""), file("a/b", b"")]).is_err());
    let link = ZipEntry {
        name: "a/link",
        bytes: b"/etc",
        mode: Some(0o120_777),
    };
    assert!(checks(&[link, file("a/link/passwd", b"")]).is_err());
}

/// A zip that is not one, or is cut short, is `unpack`.
#[test]
fn a_broken_zip_is_refused() {
    let mut cut = zip(&[file(MAC_BINARY, b"chrome")]);
    cut.truncate(cut.len() - 30);
    for bytes in [b"not a zip".to_vec(), cut] {
        let tools = Fake {
            zip: bytes,
            ..Fake::working()
        };
        fails_with("cft-broken", &tools, BrowserInstallFailure::Unpack);
    }
}

/// A link the unpack made that leads out of the folder, or is absolute, is refused.
#[cfg(unix)]
#[test]
fn an_escaping_link_after_unpacking_is_refused() {
    for target in ["../../../outside", "/etc"] {
        let tools = Fake {
            made: vec![Made::File(MAC_BINARY), Made::Link("out", target)],
            ..Fake::working()
        };
        fails_with("cft-link", &tools, BrowserInstallFailure::Unpack);
    }
    let inside = Fake {
        made: vec![Made::File(MAC_BINARY), Made::Link("in", "chrome-mac-arm64")],
        ..Fake::working()
    };
    let dir = TempDir::new("cft-link-in");
    let (result, _) = run_with(
        &job(dir.path(), Platform::MacArm64),
        &inside,
        &Cancel::new(),
    );
    result.unwrap();
}

/// A zip without the browser binary where Chrome for Testing puts it is `unpack`.
#[test]
fn a_zip_without_the_binary_is_refused() {
    let tools = Fake {
        made: vec![Made::File("chrome-mac-arm64/README")],
        ..Fake::working()
    };
    fails_with("cft-no-binary", &tools, BrowserInstallFailure::Unpack);
}

/// Too little disk is `disk_space`; Linux and Windows on ARM64 are `unsupported`; neither
/// calls a tool.
#[test]
fn low_disk_and_unsupported_machines_download_nothing() {
    let low = Fake {
        free: super::MIN_FREE_BYTES - 1,
        ..Fake::working()
    };
    fails_with("cft-disk", &low, BrowserInstallFailure::DiskSpace);
    assert_eq!(low.calls(), Vec::<&str>::new());
    for platform in [Platform::Linux, Platform::WindowsArm64, Platform::Other] {
        let dir = TempDir::new("cft-unsupported");
        let tools = Fake::working();
        let (result, _) = run_with(&job(dir.path(), platform), &tools, &Cancel::new());
        assert_eq!(result.unwrap_err().code, BrowserInstallFailure::Unsupported);
        assert_eq!(tools.calls(), Vec::<&str>::new());
    }
}

/// A failed signature check fails the download only when required; today it is logged.
#[test]
fn a_failed_signature_fails_only_when_required() {
    let failure = check_signature(true, Err("not signed".to_owned())).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::Signature);
    check_signature(false, Err("not signed".to_owned())).unwrap();
    check_signature(true, Ok(())).unwrap();
    assert!(!super::signature_required(Platform::MacArm64));
    assert!(!super::signature_required(Platform::WindowsX64));
    let unsigned = Fake {
        signed: Err("not signed".to_owned()),
        ..Fake::working()
    };
    let dir = TempDir::new("cft-unsigned");
    let (result, _) = run_with(
        &job(dir.path(), Platform::MacArm64),
        &unsigned,
        &Cancel::new(),
    );
    result.unwrap();
}

/// The start-up sweep removes every `.partial-*` folder, file and link, never a link's target,
/// and leaves the downloads and anything else.
#[test]
fn the_sweep_removes_working_folders_only() {
    let dir = TempDir::new("cft-sweep");
    let browsers = dir.path().join("browsers");
    let kept = dir.path().join("kept");
    write::create_dir_all(&kept).unwrap();
    write::write_file(&kept.join("file"), b"").unwrap();
    for folder in [".partial-1-a/chrome/x", "chrome-131.0.1.1", "other"] {
        write::create_dir_all(&browsers.join(folder)).unwrap();
    }
    write::write_file(&browsers.join(".partial-2-b"), b"").unwrap();
    #[cfg(unix)]
    {
        write::symlink(&kept, &browsers.join(".partial-3-c")).unwrap();
        write::symlink(
            &kept,
            &browsers.join(".partial-1-a").join("chrome").join("in"),
        )
        .unwrap();
    }
    archive::sweep_partials(&browsers);
    let mut left: Vec<String> = std::fs::read_dir(&browsers)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["chrome-131.0.1.1", "other"]);
    assert!(kept.join("file").is_file());
    archive::sweep_partials(&dir.path().join("missing"));
}

/// The final response's `Content-Length` from curl's `-D` file: after redirects, only once
/// its header block is complete, and only for a 2xx.
#[test]
fn content_length_is_the_final_responses() {
    let redirect = "HTTP/1.1 302 Found\r\nContent-Length: 5\r\nLocation: x\r\n\r\n";
    let ok = "HTTP/2 200\r\ncontent-length: 1234\r\n\r\n";
    assert_eq!(content_length(&format!("{redirect}{ok}")), Some(1234));
    assert_eq!(content_length(redirect), None);
    assert_eq!(
        content_length("HTTP/2 200\r\ncontent-length: 1234\r\n"),
        None
    );
    assert_eq!(content_length("HTTP/2 200\r\n\r\n"), None);
    assert_eq!(
        content_length("HTTP/2 200\r\nContent-Length: x\r\n\r\n"),
        None
    );
    assert_eq!(content_length(""), None);
}

/// curl's exit 63 (the 400 MiB cap) is `size_mismatch`, 47 (a redirect) `host_refused`; any
/// other failure `network`.
#[test]
fn curl_exits_map_to_codes() {
    curl_exit(Some(0), "").unwrap();
    assert_eq!(
        curl_exit(Some(63), "").unwrap_err().code,
        BrowserInstallFailure::SizeMismatch
    );
    assert_eq!(
        curl_exit(Some(47), "").unwrap_err().code,
        BrowserInstallFailure::HostRefused
    );
    for code in [Some(6), Some(22), Some(28), None] {
        assert_eq!(
            curl_exit(code, "x").unwrap_err().code,
            BrowserInstallFailure::Network
        );
    }
}

/// A real download from Google with the system tools, on macOS or Windows x64 only. By hand:
/// `cargo test -p clauseo-host --lib real_download -- --ignored --nocapture`.
#[test]
#[ignore = "downloads about 200 MB from Google; run by hand on macOS or Windows"]
fn real_download() {
    let dir = TempDir::new("cft-real");
    let job = Job {
        platform: Platform::current(),
        app_data: dir.path().to_owned(),
        random: "real".to_owned(),
    };
    let mut states = Vec::new();
    let result = run(&job, &super::SystemTools, &Cancel::new(), &mut |state| {
        println!("{state:?}");
        states.push(state);
    });
    let folder = result.unwrap();
    println!("installed in {}", folder.display());
    assert!(states.contains(&BrowserInstallState::Unpacking));
}
