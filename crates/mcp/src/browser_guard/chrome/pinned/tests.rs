//! Which Chrome the guard starts: the pinned download passes; a wrapper, a link to one, or an
//! installed Chrome does not.

use std::path::{Path, PathBuf};

#[cfg(windows)]
use super::modifiable;
use super::{CFT_PREFIX, Layout, SHELL_PREFIX, Trusted, check};
use crate::write;

/// The Chrome for Testing binary inside a download's folder, as the macOS zip lays it out.
const BINARY: &str = "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Chrome";

/// A fresh folder under the temp folder for one test.
fn scratch(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("clauseo-mcp-pinned-{test}-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    write::create_dir_all(&dir).unwrap();
    // The real path: on macOS the temp folder is behind a link.
    super::real_path(&dir).unwrap()
}

/// A file at `path`, its folders made.
fn file(path: &Path) -> PathBuf {
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write(path, "#!/bin/sh\n").unwrap();
    path.to_owned()
}

/// What the guard trusts with `data` as the data folder.
fn trusted(data: &Path) -> Trusted {
    Trusted {
        browsers: Some(data.join("browsers")),
        layouts: vec![layout()],
        ..Trusted::default()
    }
}

/// Chrome for Testing's layout with [`BINARY`].
fn layout() -> Layout {
    Layout {
        prefix: CFT_PREFIX,
        binary: PathBuf::from(BINARY),
    }
}

fn never(_: &Path) -> bool {
    false
}

#[test]
fn the_pinned_install_passes() {
    let dir = scratch("pinned");
    let data = dir.join("data");
    let chrome = file(&data.join("browsers/chrome-131.0.6778.85").join(BINARY));
    assert_eq!(check(&chrome, &trusted(&data), never), Some(chrome.clone()));
    // Asked through `..`, the real path is the one started.
    let roundabout = data
        .join("browsers/../browsers/chrome-131.0.6778.85")
        .join(BINARY);
    assert_eq!(check(&roundabout, &trusted(&data), never), Some(chrome));
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_wrapper_anywhere_else_is_refused() {
    let dir = scratch("wrapper");
    let data = dir.join("data");
    write::create_dir_all(&data.join("browsers")).unwrap();
    for wrapper in [
        file(&dir.join("chrome-wrapper.sh")),
        file(&dir.join("elsewhere/browsers/chrome-1/").join(BINARY)),
        file(&data.join("browsers/chrome-evil").join(BINARY)),
        file(&data.join("browsers/chrome-").join(BINARY)),
        file(&data.join("browsers/chrome-1/other-binary")),
        file(&data.join("browsers").join(BINARY)),
    ] {
        assert_eq!(check(&wrapper, &trusted(&data), never), None, "{wrapper:?}");
    }
    assert_eq!(check(Path::new("chrome"), &trusted(&data), never), None);
    assert_eq!(check(&dir.join("missing"), &trusted(&data), never), None);
    write::create_dir_all(&data.join("browsers/chrome-2").join(BINARY)).unwrap();
    let folder = data.join("browsers/chrome-2").join(BINARY);
    assert_eq!(check(&folder, &trusted(&data), never), None, "a folder");
    write::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_from_a_trusted_place_to_anywhere_else_is_refused() {
    let dir = scratch("link");
    let data = dir.join("data");
    let wrapper = file(&dir.join("tmp/chrome-wrapper.sh"));
    let link = data.join("browsers/chrome-131.0.6778.85").join(BINARY);
    write::create_dir_all(link.parent().unwrap()).unwrap();
    write::symlink(&wrapper, &link).unwrap();
    assert_eq!(check(&link, &trusted(&data), never), None);
    // A linked folder on the way is followed the same.
    let linked = data.join("browsers/chrome-132");
    write::symlink(&dir.join("tmp"), &linked).unwrap();
    file(&dir.join("tmp").join(BINARY));
    assert_eq!(check(&linked.join(BINARY), &trusted(&data), never), None);
    // A link elsewhere to the pinned install starts the install itself.
    let chrome = file(&data.join("browsers/chrome-133").join(BINARY));
    let to_chrome = dir.join("to-chrome");
    write::symlink(&chrome, &to_chrome).unwrap();
    assert_eq!(check(&to_chrome, &trusted(&data), never), Some(chrome));
    write::remove_dir_all(&dir).unwrap();
}

/// Windows: any data folder's Chrome for Testing, if the user can change nothing above it.
#[test]
fn any_data_folders_install_passes_only_where_the_user_cannot_change_it() {
    let dir = scratch("unmodifiable");
    let chrome = file(
        &dir.join("lawyer/data/browsers/chrome-131.0.6778.85")
            .join(BINARY),
    );
    let trusted = Trusted {
        any_unmodifiable_browsers: true,
        layouts: vec![layout()],
        ..Trusted::default()
    };
    assert_eq!(check(&chrome, &trusted, never), Some(chrome.clone()));
    assert_eq!(check(&chrome, &trusted, |path| path == chrome), None);
    let data = dir.join("lawyer/data");
    assert_eq!(check(&chrome, &trusted, |path| path == data), None);
    let without = Trusted {
        any_unmodifiable_browsers: false,
        ..trusted
    };
    assert_eq!(check(&chrome, &without, never), None);
    write::remove_dir_all(&dir).unwrap();
}

/// Linux (development and CI only): an installed browser passes only at its exact place.
#[test]
fn a_listed_install_passes_only_at_its_exact_place() {
    let dir = scratch("listed");
    let chrome = file(&dir.join("opt/google/chrome/chrome"));
    let trusted = Trusted {
        files: vec![chrome.clone()],
        ..Trusted::default()
    };
    assert_eq!(check(&chrome, &trusted, never), Some(chrome.clone()));
    let beside = file(&dir.join("opt/google/chrome/other"));
    assert_eq!(check(&beside, &trusted, never), None);
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_tests_folder_trusts_its_files() {
    let dir = scratch("folders");
    let stub = file(&dir.join("chrome-stub"));
    let trusted = Trusted {
        folders: vec![dir.clone()],
        ..Trusted::default()
    };
    assert_eq!(check(&stub, &trusted, never), Some(stub.clone()));
    assert_eq!(check(&stub, &Trusted::default(), never), None);
    write::remove_dir_all(&dir).unwrap();
}

/// macOS: Chrome for Testing and chrome-headless-shell under the data folder in the home given
/// (the user database's in production) pass; an installed Chrome, even in the user's own
/// `Applications`, and the same layout under any other folder are refused.
#[test]
fn on_a_mac_only_the_pinned_downloads_in_the_users_data_folder_pass() {
    let dir = scratch("machome");
    let browsers = dir.join("home/Library/Application Support/ai.clauseo.desktop/browsers");
    let cft = file(&browsers.join(
        "chrome-131.0.6778.85/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/\
         Google Chrome for Testing",
    ));
    let shell = file(&browsers.join(format!(
        "{SHELL_PREFIX}131.0.6778.85/chrome-headless-shell-mac-arm64/chrome-headless-shell"
    )));
    let trusted = Trusted::mac(Some(dir.join("home")), "mac-arm64");
    assert_eq!(check(&cft, &trusted, never), Some(cft.clone()));
    assert_eq!(check(&shell, &trusted, never), Some(shell.clone()));
    let wrong_folder = file(&browsers.join(
        "chrome-headless-shell-131.0/chrome-mac-arm64/Google Chrome for Testing.app/Contents/\
         MacOS/Google Chrome for Testing",
    ));
    let installed =
        file(&dir.join("home/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
    let lookalike = file(&dir.join("tmp/home/Library/Application Support/ai.clauseo.desktop/browsers/chrome-headless-shell-1/chrome-headless-shell-mac-arm64/chrome-headless-shell"));
    for refused in [&wrong_folder, &installed, &lookalike] {
        assert_eq!(check(refused, &trusted, never), None, "{refused:?}");
    }
    assert_eq!(check(&cft, &Trusted::mac(None, "mac-arm64"), never), None);
    write::remove_dir_all(&dir).unwrap();
}

/// Windows, run on the Windows job: a file the test wrote is its own, and an owner may always
/// rewrite its permissions, so the probe says modifiable and the check refuses it. Only that
/// direction: the CI runner is elevated, so it could modify a system install too.
#[cfg(windows)]
#[test]
fn a_file_the_user_wrote_is_modifiable_and_refused() {
    let dir = scratch("owner");
    let chrome = file(&dir.join("data/browsers/chrome-131.0.6778.85").join(BINARY));
    assert!(modifiable(&chrome));
    assert!(modifiable(chrome.parent().unwrap()));
    let trusted = Trusted {
        any_unmodifiable_browsers: true,
        layouts: vec![layout()],
        ..Trusted::default()
    };
    assert_eq!(check(&chrome, &trusted, modifiable), None);
    write::remove_dir_all(&dir).unwrap();
}
