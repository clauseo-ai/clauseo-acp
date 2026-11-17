//! Which Chrome, with one fixture folder per case of S27's "Which Chrome" (done when, item 1).

use std::path::{Path, PathBuf};

use clauseo_settings::chromium_policy::Browser;

use super::{
    ChromeRoots, EDGE_ALLOWED, MAC_BUNDLE_IDS, Platform, WindowsBrowsers, find, find_with, version,
    windows_installed, windows_installed_with,
};
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
        local_app_data: Some(dir.join(LOCAL)),
        app_data: dir.join("app"),
    }
}

/// Makes an empty file at `dir/relative` and returns its path. A macOS app's binary
/// (`<name>.app/Contents/MacOS/...`) gets its bundle's `Info.plist` with the id
/// [`MAC_BUNDLE_IDS`] expects for `<name>.app` (another app's, `other.app.id`).
fn touch(dir: &Path, relative: &str) -> PathBuf {
    let path = bare(dir, relative);
    if let Some(bundle) = path
        .ancestors()
        .nth(3)
        .filter(|b| b.extension() == Some("app".as_ref()))
    {
        let name = bundle.file_name().unwrap();
        let id = MAC_BUNDLE_IDS
            .iter()
            .find(|(app, _)| name == *app)
            .map_or("other.app.id", |(_, id)| *id);
        info_plist(bundle, id);
    }
    path
}

/// Makes an empty file at `dir/relative`, with no `Info.plist`, and returns its path.
fn bare(dir: &Path, relative: &str) -> PathBuf {
    let path = relative
        .split('/')
        .fold(dir.to_owned(), |path, part| path.join(part));
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(&path, b"").unwrap();
    path
}

/// Writes `bundle`'s `Contents/Info.plist` with `id` as its `CFBundleIdentifier`.
fn info_plist(bundle: &Path, id: &str) {
    let mut dict = plist::Dictionary::new();
    dict.insert("CFBundleIdentifier".to_owned(), id.into());
    dict.insert("CFBundleName".to_owned(), "Browser".into());
    let mut bytes = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut bytes)
        .unwrap();
    write::write_file(&bundle.join("Contents").join("Info.plist"), &bytes).unwrap();
}

const MAC_CHROME: &str = "Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const MAC_EDGE: &str = "Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge";
const MAC_BRAVE: &str = "Applications/Brave Browser.app/Contents/MacOS/Brave Browser";
const MAC_CHROMIUM: &str = "Applications/Chromium.app/Contents/MacOS/Chromium";
const WIN_CHROME: &str = "Google/Chrome/Application/chrome.exe";
const WIN_EDGE: &str = "Microsoft/Edge/Application/msedge.exe";
const CFT_MAC_ARM: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";
const CFT_WIN: &str = "chrome-win64/chrome.exe";
/// Where a per-user Chrome installs itself under `%LOCALAPPDATA%`, as a fixture's `local`.
const LOCAL: &str = "local";

#[test]
fn installed_chrome_wins_on_mac_over_home_and_chrome_for_testing() {
    let dir = TempDir::new("chrome-mac-installed");
    let system = touch(&dir.path().join("root"), MAC_CHROME);
    touch(&dir.path().join("home"), MAC_CHROME);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(system));
}

#[test]
fn mac_home_applications_comes_second() {
    let dir = TempDir::new("chrome-mac-home");
    let home = touch(&dir.path().join("home"), MAC_CHROME);
    touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(home));
}

#[test]
fn windows_chrome_in_program_files_order() {
    let dir = TempDir::new("chrome-win-order");
    touch(&dir.path().join(LOCAL), WIN_CHROME);
    let x86 = touch(&dir.path().join("pf86"), WIN_CHROME);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(x86));
    let pf = touch(&dir.path().join("pf"), WIN_CHROME);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(pf));
}

/// A per-user Chrome cannot start sandboxed over the guard's pipe, so Windows never takes one,
/// even with nothing else installed; it is found as present but not usable, so the lawyer is
/// told why (`BrowserPerUserChromeOnly`), not to install Chrome.
#[test]
fn windows_finds_a_per_user_chrome_but_never_takes_it() {
    let dir = TempDir::new("chrome-win-local");
    let local = touch(&dir.path().join(LOCAL), WIN_CHROME);
    for platform in [Platform::WindowsX64, Platform::WindowsArm64] {
        let roots = roots(dir.path(), platform);
        assert_eq!(find(&roots), None, "{platform:?}");
        assert_eq!(
            windows_installed(&roots),
            WindowsBrowsers {
                usable: Vec::new(),
                per_user_chrome: Some(local.clone()),
            },
            "{platform:?}"
        );
    }
    // Beside a Program Files Chrome it is still never taken.
    let pf = touch(&dir.path().join("pf"), WIN_CHROME);
    let installed = windows_installed(&roots(dir.path(), Platform::WindowsX64));
    assert_eq!(installed.usable, [(Browser::Chrome, pf)]);
    assert_eq!(installed.per_user_chrome, Some(local));
    // Without the folder there is none to find.
    let unset = ChromeRoots {
        local_app_data: None,
        ..roots(dir.path(), Platform::WindowsX64)
    };
    assert_eq!(windows_installed(&unset).per_user_chrome, None);
}

#[test]
fn edge_is_the_first_place_looked_on_windows_and_only_when_allowed() {
    const { assert!(EDGE_ALLOWED, "Edge is Windows' first browser") };
    let dir = TempDir::new("chrome-win-edge");
    let edge = touch(&dir.path().join("pf86"), WIN_EDGE);
    touch(&dir.path().join("pf"), WIN_EDGE);
    for platform in [Platform::WindowsX64, Platform::WindowsArm64] {
        let roots = roots(dir.path(), platform);
        assert_eq!(
            find_with(&roots, false, false),
            None,
            "{platform:?}: Edge while not allowed"
        );
        assert_eq!(find(&roots), Some(edge.clone()), "{platform:?}");
        assert_eq!(
            roots.edge(),
            [
                dir.path().join("pf86").join(WIN_EDGE),
                dir.path().join("pf").join(WIN_EDGE)
            ],
            "{platform:?}: Program Files (x86), then Program Files"
        );
    }
    // An allowed Edge comes before a Program Files Chrome; a disallowed one leaves Chrome.
    let chrome = touch(&dir.path().join("pf"), WIN_CHROME);
    let win = roots(dir.path(), Platform::WindowsX64);
    assert_eq!(find_with(&win, true, false), Some(edge.clone()));
    assert_eq!(find_with(&win, false, false), Some(chrome.clone()));
    assert_eq!(
        windows_installed(&win).usable,
        [(Browser::Edge, edge), (Browser::Chrome, chrome.clone())]
    );
    assert_eq!(
        windows_installed_with(&win, false).usable,
        [(Browser::Chrome, chrome)]
    );
}

#[test]
fn windows_installed_is_empty_off_windows() {
    let dir = TempDir::new("chrome-installed-off-windows");
    touch(&dir.path().join("pf"), WIN_CHROME);
    touch(&dir.path().join("pf86"), WIN_EDGE);
    touch(&dir.path().join("root"), MAC_CHROME);
    touch(&dir.path().join(LOCAL), WIN_CHROME);
    for platform in [Platform::MacArm64, Platform::MacX64, Platform::Linux] {
        assert_eq!(
            windows_installed(&roots(dir.path(), platform)),
            WindowsBrowsers::default()
        );
    }
}

#[test]
fn windows_edge_is_never_looked_for_on_mac() {
    let dir = TempDir::new("chrome-mac-no-edge");
    touch(&dir.path().join("pf86"), WIN_EDGE);
    assert_eq!(
        find_with(&roots(dir.path(), Platform::MacX64), true, false),
        None
    );
}

/// On a Mac with no Chrome: Edge, Brave, then Chromium, each in `/Applications` before
/// `~/Applications`; a missing one is skipped, and Chrome anywhere still wins.
#[test]
fn mac_other_chromium_browsers_come_after_chrome_in_order() {
    let dir = TempDir::new("chrome-mac-others");
    let (root, home) = (dir.path().join("root"), dir.path().join("home"));
    let mac = || roots(dir.path(), Platform::MacArm64);
    let chromium = touch(&home, MAC_CHROMIUM);
    assert_eq!(find(&mac()), Some(chromium));
    // No Edge: Brave in `~/Applications` comes before Chromium.
    let home_brave = touch(&home, MAC_BRAVE);
    assert_eq!(find(&mac()), Some(home_brave));
    let brave = touch(&root, MAC_BRAVE);
    assert_eq!(find(&mac()), Some(brave), "/Applications first");
    let edge = touch(&home, MAC_EDGE);
    assert_eq!(find(&mac()), Some(edge));
    // Windows' Edge switch does not touch the Mac's list.
    assert!(find_with(&mac(), false, false).is_some());
    let chrome = touch(&home, MAC_CHROME);
    assert_eq!(find(&mac()), Some(chrome));
    // Each browser's places, in order.
    let order: Vec<PathBuf> = [MAC_EDGE, MAC_BRAVE, MAC_CHROMIUM]
        .into_iter()
        .flat_map(|app| [root.join(app), home.join(app)])
        .collect();
    assert_eq!(mac().others(), order);
    assert_eq!(roots(dir.path(), Platform::MacX64).others(), order);
}

/// Clauseo's own Chrome for Testing is Chrome, so it comes before the Mac's other browsers.
#[test]
fn mac_chrome_for_testing_beats_the_other_browsers() {
    let dir = TempDir::new("chrome-mac-others-cft");
    touch(&dir.path().join("root"), MAC_EDGE);
    let cft = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.6778.85/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(cft));
}

/// A Mac app is taken only from its own bundle: one with the wrong id, none, or a link to a
/// real bundle is passed over, and the next place is looked at.
#[test]
fn a_mac_app_needs_its_own_bundle_id() {
    let dir = TempDir::new("chrome-mac-bundle-id");
    let (root, home) = (dir.path().join("root"), dir.path().join("home"));
    let mac = || roots(dir.path(), Platform::MacArm64);
    // Chrome's name on another app's bundle.
    let chrome = touch(&root, MAC_CHROME);
    info_plist(chrome.ancestors().nth(3).unwrap(), "com.example.notchrome");
    // A plain folder: Edge's binary and no Info.plist.
    bare(&root, MAC_EDGE);
    // No Info.plist id at all on Chromium.
    let chromium = touch(&root, MAC_CHROMIUM);
    info_plist(chromium.ancestors().nth(3).unwrap(), "");
    assert_eq!(find(&mac()), None);
    let brave = touch(&home, MAC_BRAVE);
    assert_eq!(find(&mac()), Some(brave));
}

/// A bundle that is a link, even to a real Chrome, is not taken.
#[cfg(unix)]
#[test]
fn a_linked_mac_bundle_is_passed_over() {
    let dir = TempDir::new("chrome-mac-bundle-link");
    let real = touch(&dir.path().join("elsewhere"), MAC_CHROME);
    let apps = dir.path().join("root").join("Applications");
    write::create_dir_all(&apps).unwrap();
    write::symlink(
        real.ancestors().nth(3).unwrap(),
        &apps.join("Google Chrome.app"),
    )
    .unwrap();
    assert!(dir.path().join("root").join(MAC_CHROME).is_file());
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), None);
}

/// The Mac's list is the Mac's: Windows and Linux never look there, and Arc is not on it.
#[test]
fn mac_others_are_looked_for_on_mac_alone_and_arc_never() {
    let dir = TempDir::new("chrome-mac-others-only");
    touch(
        &dir.path().join("root"),
        "Applications/Arc.app/Contents/MacOS/Arc",
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), None);
    touch(&dir.path().join("root"), MAC_BRAVE);
    for platform in [Platform::WindowsX64, Platform::Linux, Platform::Other] {
        assert_eq!(find(&roots(dir.path(), platform)), None, "{platform:?}");
    }
}

#[test]
fn chrome_for_testing_highest_version_wins() {
    const CFT_WIN: &str = CFT_MAC_ARM;
    let dir = TempDir::new("chrome-cft-version");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-99.0.9999.1/{CFT_WIN}"),
    );
    let newest = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.6778.85/{CFT_WIN}"),
    );
    touch(
        dir.path(),
        &format!("app/browsers/chrome-157.9.1/{CFT_WIN}"),
    );
    // A higher version without its binary, and folders that are no version, are passed over.
    write::create_dir_all(&dir.path().join("app/browsers/chrome-200.0.0.0")).unwrap();
    touch(dir.path(), &format!("app/browsers/chrome-latest/{CFT_WIN}"));
    touch(dir.path(), &format!("app/browsers/other-300.0/{CFT_WIN}"));
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(newest));
}

#[test]
fn chrome_for_testing_layout_per_platform() {
    let dir = TempDir::new("chrome-cft-layout");
    let arm = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(arm));
    // The arm64 zip's layout is not the x64 one.
    assert_eq!(find(&roots(dir.path(), Platform::MacX64)), None);
    let x64 = touch(
        dir.path(),
        "app/browsers/chrome-158.0.1/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacX64)), Some(x64));
}

/// Chrome for Testing in its per-user folder cannot start sandboxed over the guard's pipe, so
/// Windows never takes one, x64 or ARM64, even with nothing else installed.
#[test]
fn windows_ignores_chrome_for_testing() {
    let dir = TempDir::new("chrome-win-cft");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_WIN}"),
    );
    for platform in [Platform::WindowsX64, Platform::WindowsArm64] {
        assert_eq!(find(&roots(dir.path(), platform)), None, "{platform:?}");
        assert_eq!(platform.cft_name(), None, "{platform:?}: no download");
        assert_eq!(platform.cft_binary(), None, "{platform:?}");
    }
    // Program Files Chrome serves both.
    let chrome = touch(&dir.path().join("pf"), WIN_CHROME);
    for platform in [Platform::WindowsX64, Platform::WindowsArm64] {
        assert_eq!(find(&roots(dir.path(), platform)), Some(chrome.clone()));
    }
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

#[test]
fn windows_edge_beats_program_files_chrome_and_ignores_the_rest() {
    let dir = TempDir::new("chrome-win-edge-first");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_WIN}"),
    );
    touch(&dir.path().join(LOCAL), WIN_CHROME);
    let chrome = touch(&dir.path().join("pf86"), WIN_CHROME);
    assert_eq!(
        find(&roots(dir.path(), Platform::WindowsX64)),
        Some(chrome),
        "Program Files Chrome over a per-user one and Chrome for Testing"
    );
    let edge = touch(&dir.path().join("pf"), WIN_EDGE);
    assert_eq!(find(&roots(dir.path(), Platform::WindowsX64)), Some(edge));
}

/// A link in the browsers folder picks nothing: a linked version folder, or a binary linked to a
/// program outside the folder.
#[cfg(unix)]
#[test]
fn a_link_in_the_browsers_folder_is_passed_over() {
    const CFT_WIN: &str = CFT_MAC_ARM;
    let dir = TempDir::new("chrome-cft-links");
    let real = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.1/{CFT_WIN}"),
    );
    let elsewhere = touch(dir.path(), &format!("outside/chrome-999.0/{CFT_WIN}"));
    write::symlink(
        &dir.path().join("outside/chrome-999.0"),
        &dir.path().join("app/browsers/chrome-999.0"),
    )
    .unwrap();
    let linked = dir
        .path()
        .join(format!("app/browsers/chrome-500.0/{CFT_WIN}"));
    write::create_dir_all(linked.parent().unwrap()).unwrap();
    write::symlink(&elsewhere, &linked).unwrap();
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), Some(real));
}

/// The highest copy below this release's floor is never run, nor is an older one in its place.
#[test]
fn chrome_for_testing_below_the_floor_is_not_run() {
    let dir = TempDir::new("chrome-cft-floor");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-131.0.6778.85/{CFT_MAC_ARM}"),
    );
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), None);
}

/// A copy more than one Stable release behind the daily check's record is not run (it waits
/// for its update); Stable, or the one release behind, is. Never an older copy in its place.
#[test]
fn chrome_for_testing_more_than_one_release_behind_is_not_run() {
    use crate::browser::install::fresh::{StableRecord, write_record};
    let dir = TempDir::new("chrome-cft-behind");
    touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.0.1/{CFT_MAC_ARM}"),
    );
    let copy = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.0.2/{CFT_MAC_ARM}"),
    );
    let browsers = dir.path().join("app/browsers");
    let roots = roots(dir.path(), Platform::MacArm64);
    let record = |stable: &str, previous: Option<&str>| StableRecord {
        stable: stable.to_owned(),
        previous: previous.map(str::to_owned),
        checked_at: 0,
    };
    write_record(&browsers, &record("158.0.0.3", Some("158.0.0.2"))).unwrap();
    assert_eq!(find(&roots), Some(copy.clone()), "one release behind runs");
    write_record(&browsers, &record("158.0.0.4", Some("158.0.0.3"))).unwrap();
    assert_eq!(find(&roots), None, "two behind: neither copy runs");
    write_record(&browsers, &record("158.0.0.3", None)).unwrap();
    assert_eq!(find(&roots), None, "behind, the release before unknown");
    write_record(&browsers, &record("158.0.0.2", None)).unwrap();
    assert_eq!(find(&roots), Some(copy));
}

/// A link at `<app data>/browsers` itself is never followed: the copy it points at is not run.
#[cfg(unix)]
#[test]
fn a_linked_browsers_folder_is_not_followed() {
    let dir = TempDir::new("chrome-cft-linked-folder");
    let elsewhere = dir.path().join("elsewhere");
    let binary = touch(&elsewhere, &format!("chrome-158.0.6778.85/{CFT_MAC_ARM}"));
    write::create_dir_all(&dir.path().join("app")).unwrap();
    write::symlink(&elsewhere, &dir.path().join("app/browsers")).unwrap();
    assert!(binary.is_file());
    assert_eq!(find(&roots(dir.path(), Platform::MacArm64)), None);
}

/// The test row's switch, with the feature on: the installed Chrome, Edge, Brave and Chromium are
/// passed over, Clauseo's own download is still found, and only `1` turns it on.
#[cfg(feature = "test-browser-search")]
#[test]
fn the_test_switch_hides_installed_browsers_but_not_the_download() {
    use std::ffi::OsStr;
    let dir = TempDir::new("chrome-test-switch");
    for app in [MAC_CHROME, MAC_EDGE, MAC_BRAVE, MAC_CHROMIUM] {
        touch(&dir.path().join("root"), app);
    }
    let roots = roots(dir.path(), Platform::MacArm64);
    assert!(
        find_with(&roots, true, false).is_some(),
        "found without the switch"
    );
    assert_eq!(
        find_with(&roots, true, true),
        None,
        "Chrome, Edge, Brave and Chromium hidden, nothing else"
    );
    let cft = touch(
        dir.path(),
        &format!("app/browsers/chrome-158.0.6778.85/{CFT_MAC_ARM}"),
    );
    assert_eq!(find_with(&roots, true, true), Some(cft));
    assert!(super::test_switch::says(Some(OsStr::new("1"))));
    for value in [None, Some("0"), Some(""), Some("yes")] {
        assert!(
            !super::test_switch::says(value.map(OsStr::new)),
            "{value:?}"
        );
    }
    assert_eq!(
        super::test_switch::VAR,
        "CLAUSEO_TEST_IGNORE_INSTALLED_BROWSERS"
    );
}

/// Without the feature (every release), the variable does nothing: no value turns the switch on.
#[cfg(not(feature = "test-browser-search"))]
#[test]
fn without_the_feature_the_test_switch_is_always_off() {
    use std::ffi::OsStr;
    for value in [Some("1"), Some("0"), None] {
        assert!(
            !super::test_switch::says(value.map(OsStr::new)),
            "{value:?}"
        );
    }
    assert!(!super::test_switch::ignore_installed());
}
