//! Which Chrome, with one fixture folder per case of S27's "Which Chrome" (done when, item 1)
//! and of decision record 0029's launch checks: the exact pinned version only, its folder
//! against its install record, and never a Chrome installed on the machine.

use std::path::{Path, PathBuf};

use clauseo_protocol::ToolsUnavailableReason;

use super::{ChromeRoots, Picked, Platform, find_checked, version};
use crate::browser::install::record;
use crate::browser::pins::Pins;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// Roots for `platform` with every folder under the fixture `dir`.
fn roots(dir: &Path, platform: Platform) -> ChromeRoots {
    ChromeRoots {
        platform,
        root: dir.join("root"),
        app_data: dir.join("app"),
    }
}

/// Makes a file at `dir/relative` holding `bytes` and returns its path.
fn put(dir: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = relative
        .split('/')
        .fold(dir.to_owned(), |path, part| path.join(part));
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(&path, bytes).unwrap();
    path
}

/// Makes an empty file at `dir/relative` and returns its path.
fn touch(dir: &Path, relative: &str) -> PathBuf {
    put(dir, relative, b"")
}

const MAC_CHROME: &str = "Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const WIN_CHROME: &str = "Google/Chrome/Application/chrome.exe";
const CFT_MAC_ARM: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";
const CFT_WIN: &str = "chrome-win64/chrome.exe";

/// The version every pin here names.
const PINNED: &str = "157.0.8103.38";

/// The zip sha256 the records here carry: never compared at launch.
const ZIP_SHA: &str = "ae0deef27d449e1d7529df7705d609bab102de48464da13567ed3dd2ee1a0586";

/// Two days after [`pins`]' date.
fn now() -> clauseo_protocol::Timestamp {
    crate::browser::pins::tests::at("2026-11-09T06:00:00Z")
}

/// Pins of `version` with no download in them: only their version and date are read here.
fn pins(version: &str) -> Pins {
    Pins::for_tests("2026-11-07T06:00:00Z", version, &[])
}

/// The date [`pins`] carry.
fn published() -> clauseo_blocklist::Generated {
    clauseo_blocklist::Generated::parse("2026-11-07T06:00:00Z").unwrap()
}

/// Installs a fake Chrome for Testing of `version` for `platform` under `dir/app`, as the
/// download leaves it: the binary, a library and a resource, and its install record. Returns
/// the binary's path.
fn install(dir: &Path, platform: Platform, version: &str) -> PathBuf {
    let binary = match platform {
        Platform::MacArm64 => CFT_MAC_ARM,
        Platform::MacX64
        | Platform::WindowsX64
        | Platform::WindowsArm64
        | Platform::Linux
        | Platform::Other => CFT_WIN,
    };
    let folder = dir.join("app/browsers").join(format!("chrome-{version}"));
    let path = put(&folder, binary, b"the browser");
    let top = binary.split('/').next().unwrap();
    put(&folder, &format!("{top}/libGLESv2.dll"), b"a library");
    put(&folder, &format!("{top}/resources.pak"), b"resources");
    let name = platform.cft_name().unwrap();
    record::write(&dir.join("app"), name, version, ZIP_SHA, &folder).unwrap();
    path
}

#[test]
fn the_pinned_chrome_for_testing_that_matches_its_record_is_picked() {
    for platform in [Platform::WindowsX64, Platform::MacArm64] {
        let dir = TempDir::new("chrome-pinned");
        let binary = install(dir.path(), platform, PINNED);
        assert_eq!(
            find_checked(&roots(dir.path(), platform), now(), || Some(pins(PINNED))),
            Ok(Picked {
                path: binary,
                pins_published: Some(published()),
            }),
            "{platform:?}"
        );
    }
}

#[test]
fn a_folder_above_the_pin_is_refused() {
    let dir = TempDir::new("chrome-above-pin");
    let roots = roots(dir.path(), Platform::WindowsX64);
    install(dir.path(), Platform::WindowsX64, "158.0.1.1");
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserOutOfDate)
    );
    // With the pinned one beside it, the pinned one is picked, never the higher.
    let pinned = install(dir.path(), Platform::WindowsX64, PINNED);
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))).map(|picked| picked.path),
        Ok(pinned)
    );
}

#[test]
fn the_whole_version_is_judged_not_the_major() {
    let dir = TempDir::new("chrome-full-version");
    let roots = roots(dir.path(), Platform::WindowsX64);
    // The same major, one build off, below and above: neither is the pin.
    for version in [
        "157.0.8103.37",
        "157.0.8103.39",
        "157.0.8103.38.0",
        "156.0.7000.1",
    ] {
        install(dir.path(), Platform::WindowsX64, version);
    }
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserOutOfDate)
    );
}

#[test]
fn a_swapped_main_binary_is_refused() {
    let dir = TempDir::new("chrome-swapped");
    let roots = roots(dir.path(), Platform::WindowsX64);
    let binary = install(dir.path(), Platform::WindowsX64, PINNED);
    // The same size, other bytes: only the hash tells.
    write::write_file(&binary, b"the program").unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
    // Another size: the listing tells.
    write::write_file(&binary, b"another program").unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[test]
fn an_added_file_is_refused() {
    let dir = TempDir::new("chrome-added");
    let roots = roots(dir.path(), Platform::WindowsX64);
    install(dir.path(), Platform::WindowsX64, PINNED);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-{PINNED}/chrome-win64/version.dll"),
    );
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[test]
fn a_removed_file_is_refused() {
    let dir = TempDir::new("chrome-removed");
    let roots = roots(dir.path(), Platform::WindowsX64);
    install(dir.path(), Platform::WindowsX64, PINNED);
    let folder = dir
        .path()
        .join(format!("app/browsers/chrome-{PINNED}/chrome-win64"));
    write::remove_file(&folder.join("resources.pak")).unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[cfg(unix)]
#[test]
fn a_changed_mode_is_refused() {
    let dir = TempDir::new("chrome-mode");
    let roots = roots(dir.path(), Platform::WindowsX64);
    let binary = install(dir.path(), Platform::WindowsX64, PINNED);
    write::set_mode(&binary, 0o4755).unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[test]
fn a_pinned_folder_without_its_record_or_with_another_versions_is_refused() {
    let dir = TempDir::new("chrome-no-record");
    let roots = roots(dir.path(), Platform::WindowsX64);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-{PINNED}/{CFT_WIN}"),
    );
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
    // The record names the version installed last; the pinned folder is not it.
    install(dir.path(), Platform::WindowsX64, "156.0.7000.1");
    assert_eq!(
        find_checked(&roots, now(), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
    // An unreadable record refuses too.
    write::write_file(&record::record_path(&dir.path().join("app")), b"{").unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins("156.0.7000.1"))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[test]
fn a_downloaded_chrome_without_verified_pins_is_list_unavailable() {
    let dir = TempDir::new("chrome-checked-no-pins");
    install(dir.path(), Platform::WindowsX64, PINNED);
    assert_eq!(
        find_checked(&roots(dir.path(), Platform::WindowsX64), now(), || None),
        Err(ToolsUnavailableReason::BrowserListUnavailable)
    );
}

#[test]
fn a_downloaded_chrome_under_a_list_over_45_days_old_is_expired() {
    use crate::browser::pins::tests::at;
    let dir = TempDir::new("chrome-checked-expired");
    let roots = roots(dir.path(), Platform::WindowsX64);
    install(dir.path(), Platform::WindowsX64, PINNED);
    // The pins are dated 2026-11-07T06:00:00Z. 44 days: offered, with the old list's date.
    let picked = find_checked(&roots, at("2026-12-21T06:00:00Z"), || Some(pins(PINNED)));
    assert!(picked.unwrap().pins_published.is_some());
    // 46 days: refused, even at the pinned version.
    assert_eq!(
        find_checked(&roots, at("2026-12-23T06:00:00Z"), || Some(pins(PINNED))),
        Err(ToolsUnavailableReason::BrowserListExpired)
    );
}

#[test]
fn chrome_or_edge_installed_on_the_machine_is_never_picked() {
    let dir = TempDir::new("chrome-installed-never");
    let root = dir.path().join("root");
    touch(&root, MAC_CHROME);
    touch(&root, WIN_CHROME);
    touch(&root, &format!("Program Files/{WIN_CHROME}"));
    for platform in [
        Platform::MacArm64,
        Platform::MacX64,
        Platform::WindowsX64,
        Platform::WindowsArm64,
    ] {
        let roots = roots(dir.path(), platform);
        assert_eq!(
            find_checked(&roots, now(), || panic!("read the pins")),
            Err(ToolsUnavailableReason::BrowserNotInstalled),
            "{platform:?}"
        );
    }
}

#[test]
fn nothing_downloaded_is_not_installed_and_reads_no_pins() {
    let dir = TempDir::new("chrome-checked-none");
    for platform in [
        Platform::MacArm64,
        Platform::MacX64,
        Platform::WindowsX64,
        Platform::WindowsArm64,
        Platform::Other,
    ] {
        assert_eq!(
            find_checked(&roots(dir.path(), platform), now(), || panic!(
                "read the pins"
            )),
            Err(ToolsUnavailableReason::BrowserNotInstalled),
            "{platform:?}"
        );
    }
}

#[test]
fn windows_arm64_gets_no_chrome_for_testing() {
    let dir = TempDir::new("chrome-win-arm64");
    install(dir.path(), Platform::WindowsX64, PINNED);
    assert_eq!(
        find_checked(&roots(dir.path(), Platform::WindowsArm64), now(), || Some(
            pins(PINNED)
        )),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

#[test]
fn the_mac_layout_is_per_platform() {
    let dir = TempDir::new("chrome-cft-layout");
    install(dir.path(), Platform::MacArm64, PINNED);
    // The arm64 zip's layout is not the x64 one, and the record names arm64.
    assert_eq!(
        find_checked(&roots(dir.path(), Platform::MacX64), now(), || Some(pins(
            PINNED
        ))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}

/// Linux, development and CI only: the system's Google Chrome, then Chromium, unpinned.
#[cfg(target_os = "linux")]
#[test]
fn linux_takes_google_chrome_then_chromium_unpinned() {
    let dir = TempDir::new("chrome-linux");
    let roots = roots(dir.path(), Platform::Linux);
    let none = || -> Option<Pins> { panic!("read the pins") };
    assert_eq!(
        find_checked(&roots, now(), none),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
    let chromium = touch(&dir.path().join("root"), "usr/bin/chromium");
    let picked = |path| Picked {
        path,
        pins_published: None,
    };
    assert_eq!(find_checked(&roots, now(), none), Ok(picked(chromium)));
    let chrome = touch(&dir.path().join("root"), "usr/bin/google-chrome");
    assert_eq!(find_checked(&roots, now(), none), Ok(picked(chrome)));
}

#[test]
fn versions_compare_by_number() {
    assert!(version("131.0.6778.85") > version("99.0.9999.1"));
    assert_eq!(version("1.2"), Some(vec![1, 2]));
    for bad in ["", "1..2", "1.x", "v1", "1.2 ", "-1"] {
        assert_eq!(version(bad), None, "{bad:?}");
    }
}

/// A link in the browsers folder picks nothing: a linked version folder, or a binary linked to a
/// program outside the folder.
#[cfg(unix)]
#[test]
fn a_link_in_the_browsers_folder_is_passed_over() {
    let dir = TempDir::new("chrome-cft-links");
    let roots = roots(dir.path(), Platform::WindowsX64);
    // A pinned folder that is a link to a real install elsewhere.
    let elsewhere = TempDir::new("chrome-cft-links-elsewhere");
    install(elsewhere.path(), Platform::WindowsX64, PINNED);
    write::create_dir_all(&dir.path().join("app/browsers")).unwrap();
    write::symlink(
        &elsewhere
            .path()
            .join(format!("app/browsers/chrome-{PINNED}")),
        &dir.path().join(format!("app/browsers/chrome-{PINNED}")),
    )
    .unwrap();
    assert_eq!(
        find_checked(&roots, now(), || panic!("read the pins")),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
    // A binary swapped for a link to a program outside.
    let binary = install(dir.path(), Platform::WindowsX64, "1.0");
    write::remove_file(&binary).unwrap();
    let outside = touch(dir.path(), "outside/chrome.exe");
    write::symlink(&outside, &binary).unwrap();
    assert_eq!(
        find_checked(&roots, now(), || Some(pins("1.0"))),
        Err(ToolsUnavailableReason::BrowserNotInstalled)
    );
}
