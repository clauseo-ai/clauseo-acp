//! Which Chrome the guard starts: the pinned download passes; a wrapper, a link to one, or an
//! installed Chrome does not.

use std::path::{Path, PathBuf};

use super::{CFT_PREFIX, Layout, SHELL_PREFIX, Trusted, check, is_sid_text};
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

/// Only the anchored data folder's `browsers` passes (Windows: `--data-dir`'s), never another
/// folder named `browsers`; none passes without one; and the agent able to change, or owning,
/// the binary or any folder above it, the root included, refuses it.
#[test]
fn only_the_anchored_data_folders_install_passes_and_only_where_the_agent_cannot_change_it() {
    let dir = scratch("anchored");
    let data = dir.join("lawyer/data");
    let chrome = file(&data.join("browsers/chrome-131.0.6778.85").join(BINARY));
    let other = file(
        &dir.join("agent/data/browsers/chrome-131.0.6778.85")
            .join(BINARY),
    );
    assert_eq!(check(&chrome, &trusted(&data), never), Some(chrome.clone()));
    assert_eq!(check(&other, &trusted(&data), never), None);
    let unanchored = Trusted {
        layouts: vec![layout()],
        ..Trusted::default()
    };
    assert_eq!(check(&chrome, &unanchored, never), None);
    for owned in [
        chrome.clone(),
        data.clone(),
        PathBuf::from(chrome.ancestors().last().unwrap()),
    ] {
        assert_eq!(
            check(&chrome, &trusted(&data), |path| path == owned),
            None,
            "{owned:?}"
        );
    }
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sid_reads_as_one_only_in_its_own_form() {
    for good in ["S-1-5-18", "S-1-5-21-1-2-3-1001", "S-1-15-2-1-2-3-4-5-6-7"] {
        assert!(is_sid_text(good), "{good}");
    }
    let long = format!("S-1-{}", "1-".repeat(100));
    for bad in [
        "",
        "S-1-",
        "S-1-5-",
        "s-1-5-18",
        "S-2-5",
        "S-1-5-x",
        "S-1--5",
        long.as_str(),
    ] {
        assert!(!is_sid_text(bad), "{bad}");
    }
}

/// Linux (test builds only): an installed browser passes only at its exact place, and only as
/// an ELF binary, never a script (Arch's `/usr/bin/chromium` reads a flags file).
#[test]
fn a_listed_install_passes_only_at_its_exact_place_and_as_a_binary() {
    let dir = scratch("listed");
    let chrome = dir.join("opt/google/chrome/chrome");
    file(&chrome);
    write::write(&chrome, b"\x7fELF\x02\x01\x01").unwrap();
    let script = file(&dir.join("usr/bin/chromium"));
    let trusted = Trusted {
        files: vec![chrome.clone(), script.clone()],
        ..Trusted::default()
    };
    assert_eq!(check(&chrome, &trusted, never), Some(chrome.clone()));
    assert_eq!(check(&script, &trusted, never), None, "a script");
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

/// Windows, run on the Windows job: what the test wrote is the guard token's own, so its owner
/// check refuses it, whoever else may write it; a system file owned by `TrustedInstaller` is not.
#[cfg(windows)]
#[test]
fn a_file_the_agent_owns_is_modifiable_and_refused() {
    use super::{Agent, authz::Writers, modifiable_by};
    let dir = scratch("owner");
    let data = dir.join("data");
    let chrome = file(&data.join("browsers/chrome-131.0.6778.85").join(BINARY));
    let writers = Writers::new(&Agent::default()).unwrap();
    assert!(writers.owns(&chrome));
    assert!(writers.owns(chrome.parent().unwrap()));
    let system = Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\kernel32.dll");
    assert!(!writers.owns(&system), "TrustedInstaller's");
    assert_eq!(
        check(&chrome, &trusted(&data), modifiable_by(&Agent::default())),
        None
    );
    write::remove_dir_all(&dir).unwrap();
}

/// Windows: an agent that locks its own launcher with an `OWNER RIGHTS` entry granting itself
/// only reading (so no access check grants it a write) is still refused, as its owner.
#[cfg(windows)]
#[test]
fn an_owner_rights_lock_down_does_not_pass() {
    use super::{Agent, authz, modifiable_by};
    let dir = scratch("ownerrights");
    let data = dir.join("data");
    let chrome = file(&data.join("browsers/chrome-131.0.6778.85").join(BINARY));
    // OW: OWNER RIGHTS (S-1-3-4), FR: file read. The folder above keeps its rights, so the
    // test can still delete the file afterwards.
    authz::set_dacl(&chrome, "D:P(A;;FR;;;OW)");
    let writers = authz::Writers::new(&Agent::default()).unwrap();
    assert!(writers.owns(&chrome));
    assert!(modifiable_by(&Agent::default())(&chrome));
    assert_eq!(
        check(&chrome, &trusted(&data), modifiable_by(&Agent::default())),
        None
    );
    write::remove_dir_all(&dir).unwrap();
}

/// Windows: with `--agent-sid` (the test's own user SID standing in for the agent's account,
/// `AuthzInitializeContextFromSid` and the groups Windows gives it), a folder above the binary
/// that the agent's user or group owns is refused, as is a path it may write.
#[cfg(windows)]
#[test]
fn an_ancestor_the_agent_sid_owns_is_refused() {
    use super::authz::{Writers, own_user_sid};
    let dir = scratch("agentsid");
    let data = dir.join("data");
    let chrome = file(&data.join("browsers/chrome-131.0.6778.85").join(BINARY));
    let sid = own_user_sid();
    let agent = Writers::only_sid(&sid, None).unwrap();
    assert!(
        agent.owns(&data),
        "the agent's user or one of its groups owns it"
    );
    let modifiable = |path: &Path| agent.owns(path) || agent.can_change(path);
    assert_eq!(check(&chrome, &trusted(&data), modifiable), None);
    let system = Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\kernel32.dll");
    assert!(!agent.owns(&system));
    assert!(
        Writers::only_sid("S-1-5-21-0-0-0-not", None).is_none(),
        "not a SID: fail closed"
    );
    write::remove_dir_all(&dir).unwrap();
}
