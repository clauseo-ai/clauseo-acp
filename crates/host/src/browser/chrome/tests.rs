//! Which Chrome, with one fixture folder per case of S27's "Which Chrome" (done when, item 1).

use std::path::{Path, PathBuf};

use super::{ChromeRoots, EDGE_ALLOWED, Platform, find, find_with, version};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// Roots for `platform` with every folder under the fixture `dir`.
fn roots(dir: &Path, platform: Platform) -> ChromeRoots {
    ChromeRoots {
        platform,
        root: dir.join("root"),
        home: Some(dir.join("home")),
        program_files: Some(dir.join("pf")),
        program_files_x86: Some(dir.join("pf86")),
        local_app_data: Some(dir.join("local")),
        app_data: dir.join("app"),
    }
}

/// Makes an empty file at `dir/relative` and returns its path.
fn touch(dir: &Path, relative: &str) -> PathBuf {
    let path = relative
        .split('/')
        .fold(dir.to_owned(), |path, part| path.join(part));
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(&path, b"").unwrap();
    path
}

const MAC_CHROME: &str = "Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const WIN_CHROME: &str = "Google/Chrome/Application/chrome.exe";
const WIN_EDGE: &str = "Microsoft/Edge/Application/msedge.exe";
const CFT_MAC_ARM: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";
const CFT_WIN: &str = "chrome-win64/chrome.exe";

#[test]
fn installed_chrome_wins_on_mac_over_home_and_chrome_for_testing() {
    let dir = TempDir::new("chrome-mac-installed");
    let system = touch(&dir.path().join("root"), MAC_CHROME);
    touch(&dir.path().join("home"), MAC_CHROME);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(system));
}

#[test]
fn mac_home_applications_comes_second() {
    let dir = TempDir::new("chrome-mac-home");
    let home = touch(&dir.path().join("home"), MAC_CHROME);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(home));
}

#[test]
fn windows_chrome_in_program_files_order() {
    let dir = TempDir::new("chrome-win-order");
    touch(&dir.path().join("local"), WIN_CHROME);
    let x86 = touch(&dir.path().join("pf86"), WIN_CHROME);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(x86));
    let pf = touch(&dir.path().join("pf"), WIN_CHROME);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(pf));
}

#[test]
fn windows_local_app_data_chrome() {
    let dir = TempDir::new("chrome-win-local");
    let local = touch(&dir.path().join("local"), WIN_CHROME);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(local));
}

#[test]
fn edge_is_used_only_when_allowed() {
    const { assert!(!EDGE_ALLOWED, "Edge waits for G17 item 4") };
    let dir = TempDir::new("chrome-win-edge");
    let edge = touch(&dir.path().join("pf86"), WIN_EDGE);
    touch(&dir.path().join("pf"), WIN_EDGE);
    for platform in [Platform::WindowsX64, Platform::WindowsArm64] {
        let roots = roots(dir.path(), platform);
        assert_eq!(find(&roots), None, "{platform:?}: Edge while not allowed");
        assert_eq!(find_with(&roots, true), Some(edge.clone()), "{platform:?}");
    }
    // Chrome still comes before an allowed Edge.
    let chrome = touch(&dir.path().join("local"), WIN_CHROME);
    assert_eq!(
        find_with(&roots(dir.path(), Platform::WindowsX64), true),
        Some(chrome)
    );
}

#[test]
fn edge_is_never_looked_for_on_mac() {
    let dir = TempDir::new("chrome-mac-no-edge");
    touch(&dir.path().join("pf86"), WIN_EDGE);
    assert_eq!(find_with(&roots(dir.path(), Platform::MacX64), true), None);
}

#[test]
fn chrome_for_testing_highest_version_wins() {
    let dir = TempDir::new("chrome-cft-version");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-99.0.9999.1/{CFT_WIN}"),
    );
    let newest = touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.6778.85/{CFT_WIN}"),
    );
    touch(
        dir.path(),
        &format!("app/browsers/chrome-130.9.1/{CFT_WIN}"),
    );
    // A higher version without its binary, and folders that are no version, are passed over.
    write::create_dir_all(&dir.path().join("app/browsers/chrome-200.0.0.0")).unwrap();
    touch(dir.path(), &format!("app/browsers/chrome-latest/{CFT_WIN}"));
    touch(dir.path(), &format!("app/browsers/other-300.0/{CFT_WIN}"));
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(newest));
}

#[test]
fn chrome_for_testing_layout_per_platform() {
    let dir = TempDir::new("chrome-cft-layout");
    let arm = touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(arm));
    // The arm64 zip's layout is not the x64 one.
    assert_eq!(find(&roots(dir.path(), Platform::MacX64)), None);
    let x64 = touch(
        dir.path(),
        "app/browsers/chrome-131.0.1/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacX64)), Some(x64));
}

#[test]
fn windows_arm64_skips_chrome_for_testing() {
    let dir = TempDir::new("chrome-win-arm64");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.1/{CFT_WIN}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::WindowsArm64)), None);
    assert!(find(&roots(dir.path(), Platform::WindowsX64)).is_some());
    // Installed Chrome still serves ARM64.
    let chrome = touch(&dir.path().join("pf"), WIN_CHROME);
    assert_eq!(
        find(&roots(dir.path(), Platform::WindowsArm64)),
        Some(chrome)
    );
}

#[test]
fn nothing_found_is_none() {
    let dir = TempDir::new("chrome-none");
    for platform in [
        Platform::MacArm64,
        Platform::MacX64,
        Platform::WindowsX64,
        Platform::WindowsArm64,
        Platform::Linux,
        Platform::Other,
    ] {
        assert_eq!(find(&roots(dir.path(), platform)), None, "{platform:?}");
    }
}

#[test]
fn linux_google_chrome_then_chromium() {
    let dir = TempDir::new("chrome-linux");
    let chromium = touch(&dir.path().join("root"), "usr/bin/chromium");
    assert_eq!(find(&roots(dir.path(), Platform::Linux)), Some(chromium));
    let chrome = touch(&dir.path().join("root"), "usr/bin/google-chrome");
    assert_eq!(find(&roots(dir.path(), Platform::Linux)), Some(chrome));
}

#[test]
fn a_folder_where_the_binary_should_be_is_not_chrome() {
    let dir = TempDir::new("chrome-folder");
    write::create_dir_all(
        &dir.path()
            .join("pf")
            .join("Google/Chrome/Application/chrome.exe"),
    )
    .unwrap();
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), None);
}

#[test]
fn versions_compare_by_number() {
    assert!(version("131.0.6778.85") > version("99.0.9999.1"));
    assert_eq!(version("1.2"), Some(vec![1, 2]));
    for bad in ["", "1..2", "1.x", "v1", "1.2 ", "-1"] {
        assert_eq!(version(bad), None, "{bad:?}");
    }
}
