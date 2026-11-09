//! Where the profile finds the stetdoc a release bundles in its own folder (U13), and when it
//! falls back to `PATH` or runs none.

use std::path::{Path, PathBuf};

use super::super::{bundled_stetdoc, stetdoc_program};
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
use super::STUB;
use crate::test_support::Scratch;

/// stetdoc's file name with the OS's executable suffix.
fn stetdoc_file() -> String {
    format!("stetdoc{}", std::env::consts::EXE_SUFFIX)
}

#[test]
fn on_macos_the_bundled_stetdoc_is_in_its_own_folder_under_contents_helpers() {
    let macos = PathBuf::from("/Applications/Clauseo.app/Contents/MacOS");
    let want =
        PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/stetdoc").join(stetdoc_file());
    let resources = PathBuf::from("/Applications/Clauseo.app/Contents/Resources");
    assert_eq!(
        bundled_stetdoc(true, &macos, Some(&resources)),
        Some(want.clone())
    );
    assert_eq!(bundled_stetdoc(true, &macos, None), Some(want));
    // An exe in the root has no Contents folder to look in.
    assert_eq!(bundled_stetdoc(true, Path::new("/"), None), None);
}

#[test]
fn elsewhere_the_bundled_stetdoc_is_in_its_own_folder_under_the_resource_folder() {
    let exe_dir = PathBuf::from("/usr/bin");
    let resources = PathBuf::from("/usr/lib/Clauseo");
    assert_eq!(
        bundled_stetdoc(false, &exe_dir, Some(&resources)),
        Some(resources.join("stetdoc").join(stetdoc_file()))
    );
    // On Windows the resource folder is the install folder: stetdoc\stetdoc.exe beside Clauseo.exe.
    let install = PathBuf::from("/Users/me/AppData/Local/Clauseo");
    assert_eq!(
        bundled_stetdoc(false, &install, Some(&install)),
        Some(install.join("stetdoc").join(stetdoc_file()))
    );
    assert_eq!(bundled_stetdoc(false, &exe_dir, None), None);
}

/// A bundled stetdoc that is there wins in both builds, over any on `PATH`. The test binary
/// stands in for it: a program on every OS.
#[test]
fn a_bundled_stetdoc_that_is_there_is_used_in_release_and_debug_builds() {
    let program = std::env::current_exe().unwrap();
    let path = std::env::join_paths([program.parent().unwrap()]).unwrap();
    for release in [true, false] {
        assert_eq!(
            stetdoc_program(Some(&program), Some(&path), release),
            Some(program.clone()),
            "release: {release}"
        );
    }
}

#[test]
fn a_release_build_without_the_bundled_stetdoc_runs_none() {
    let scratch = Scratch::new("profile-bundled-missing", &["bin"]);
    let missing = scratch.path().join("stetdoc").join(stetdoc_file());
    let path = std::env::join_paths([scratch.dir("bin")]).unwrap();
    assert_eq!(stetdoc_program(Some(&missing), Some(&path), true), None);
    assert_eq!(stetdoc_program(None, Some(&path), true), None);
    // A file there that is not a program (no execute bit) is not run either.
    #[cfg(unix)]
    {
        let plain = scratch.file("stetdoc-plain", b"not a program");
        assert_eq!(stetdoc_program(Some(&plain), Some(&path), true), None);
    }
}

/// Not on macOS, where `Scratch::executable` is not built.
#[cfg(all(unix, not(target_os = "macos"), debug_assertions))]
#[test]
fn a_debug_build_without_the_bundled_stetdoc_takes_the_one_on_path() {
    let scratch = Scratch::new("profile-bundled-debug", &["bin"]);
    let on_path = scratch.file("bin/stetdoc", STUB);
    scratch.executable("bin/stetdoc");
    let missing = scratch.path().join("stetdoc").join("stetdoc");
    let path = std::env::join_paths([scratch.dir("bin")]).unwrap();
    assert_eq!(
        stetdoc_program(Some(&missing), Some(&path), false),
        Some(on_path.clone())
    );
    assert_eq!(stetdoc_program(None, Some(&path), false), Some(on_path));
    assert_eq!(stetdoc_program(Some(&missing), None, false), None);
}
