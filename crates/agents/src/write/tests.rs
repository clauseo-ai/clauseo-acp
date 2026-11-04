//! The folder maker's link rule and modes (S18 security review, drift: folders follow links).

use super::{TestDir, create_private_dirs_under};

/// A missing folder and its missing parents are made under the data folder; one already there
/// is fine.
#[test]
fn makes_missing_folders_under_the_data_folder() {
    let data = TestDir::new("dirs-make");
    let target = data.0.join("agents").join("codex-acp").join("home");
    create_private_dirs_under(&data.0, &target).unwrap();
    assert!(std::fs::symlink_metadata(&target).unwrap().is_dir());
    create_private_dirs_under(&data.0, &target).unwrap();
}

/// A link below the data folder is refused and never followed: nothing is made where it
/// points.
#[cfg(unix)]
#[test]
fn refuses_a_linked_component_below_the_data_folder() {
    let data = TestDir::new("dirs-link");
    let elsewhere = TestDir::new("dirs-link-target");
    std::fs::create_dir_all(data.0.join("agents")).unwrap();
    super::symlink(&elsewhere.0, &data.0.join("agents").join("codex-acp")).unwrap();
    let target = data.0.join("agents").join("codex-acp").join("home");
    let err = create_private_dirs_under(&data.0, &target).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
    assert!(!elsewhere.0.join("home").exists());
    // A link standing at the target itself is refused too.
    let at_target = data.0.join("agents").join("claude-acp");
    super::symlink(&elsewhere.0, &at_target).unwrap();
    assert!(create_private_dirs_under(&data.0, &at_target).is_err());
}

/// A file where a folder goes is refused, and a target outside the data folder too.
#[test]
fn refuses_a_file_in_the_way_and_a_target_outside() {
    let data = TestDir::new("dirs-file");
    data.file("agents", b"");
    assert!(create_private_dirs_under(&data.0, &data.0.join("agents").join("x")).is_err());
    let other = TestDir::new("dirs-outside");
    assert!(create_private_dirs_under(&data.0, &other.0.join("x")).is_err());
    assert!(!other.0.join("x").exists());
}

/// The data folder itself may be a link (`CLAUSEO_DATA_DIR` may name one).
#[cfg(unix)]
#[test]
fn follows_a_linked_data_folder() {
    let real = TestDir::new("dirs-real");
    let holder = TestDir::new("dirs-holder");
    let data = holder.0.join("data");
    super::symlink(&real.0, &data).unwrap();
    create_private_dirs_under(&data, &data.join("agents").join("signin")).unwrap();
    assert!(real.0.join("agents").join("signin").is_dir());
}

/// Each folder it makes is 0700 on Unix, whatever the umask.
#[cfg(unix)]
#[test]
fn new_folders_are_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let data = TestDir::new("dirs-mode");
    let target = data.0.join("agents").join("codex-acp").join("home");
    create_private_dirs_under(&data.0, &target).unwrap();
    for folder in [
        data.0.join("agents"),
        data.0.join("agents").join("codex-acp"),
        target,
    ] {
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", folder.display());
    }
}

/// S18 re-review, Low: a real folder already there is set to 0700 too, as host's `make_home`
/// does; a link is refused, never chmodded through.
#[cfg(unix)]
#[test]
fn an_existing_folder_is_made_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let data = TestDir::new("dirs-premade");
    let home = data.0.join("agents").join("codex-acp").join("home");
    std::fs::create_dir_all(&home).unwrap();
    for folder in [
        data.0.join("agents"),
        home.parent().unwrap().to_owned(),
        home.clone(),
    ] {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    create_private_dirs_under(&data.0, &home).unwrap();
    let mode = std::fs::metadata(&home).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let mode = std::fs::metadata(data.0.join("agents"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}
