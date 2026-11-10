//! Where the profile finds the bundled `clauseo-agent-browser`: `Contents/Helpers` on macOS,
//! beside the executable elsewhere.

use std::path::PathBuf;

use clauseo_settings::Locations;

use super::super::{agent_browser_path, paths_for};
use crate::test_support::Scratch;

/// The helper's name with the OS's executable suffix.
fn helper() -> String {
    format!("clauseo-agent-browser{}", std::env::consts::EXE_SUFFIX)
}

#[test]
fn on_macos_the_helper_in_contents_helpers_is_preferred_over_one_beside_the_exe() {
    let macos = PathBuf::from("/Applications/Clauseo.app/Contents/MacOS");
    let helpers = PathBuf::from("/Applications/Clauseo.app/Contents/Helpers").join(helper());
    assert_eq!(agent_browser_path(&macos, true, |_| true), helpers);
    assert_eq!(
        agent_browser_path(&macos, true, |path| path == helpers),
        helpers
    );
}

#[test]
fn on_macos_without_contents_helpers_the_helper_beside_the_exe_is_used() {
    let macos = PathBuf::from("/Applications/Clauseo.app/Contents/MacOS");
    let beside = macos.join(helper());
    assert_eq!(agent_browser_path(&macos, true, |_| false), beside);
    assert_eq!(
        agent_browser_path(&macos, true, |path| path == beside),
        beside
    );
    // An exe in the root has no Contents folder to look in.
    let root = PathBuf::from("/");
    assert_eq!(
        agent_browser_path(&root, true, |_| true),
        root.join(helper())
    );
}

#[test]
fn elsewhere_the_helper_is_beside_the_exe_even_when_a_helpers_folder_has_one() {
    let dir = PathBuf::from("/opt/clauseo/bin");
    let asked = std::cell::Cell::new(false);
    let found = agent_browser_path(&dir, false, |_| {
        asked.set(true);
        true
    });
    assert_eq!(found, dir.join(helper()));
    assert!(!asked.get(), "only macOS looks in Helpers");
}

#[test]
fn the_profile_takes_the_helper_from_contents_helpers_on_macos_only() {
    let scratch = Scratch::new(
        "profile-helpers",
        &["Clauseo.app/Contents/MacOS", "Clauseo.app/Contents/Helpers"],
    );
    let helpers = scratch.file(&format!("Clauseo.app/Contents/Helpers/{}", helper()), b"");
    let beside = scratch.file(&format!("Clauseo.app/Contents/MacOS/{}", helper()), b"");
    let locations = Locations::under_root(scratch.path());
    let exe = scratch.path().join("Clauseo.app/Contents/MacOS/Clauseo");
    let stetdoc = scratch.path().join("bin").join("stetdoc").join("stetdoc");
    let paths = paths_for(&locations, exe, Some(stetdoc)).unwrap();
    let want = if cfg!(target_os = "macos") {
        helpers
    } else {
        beside
    };
    assert_eq!(paths.browser().agent_browser, want);
}
