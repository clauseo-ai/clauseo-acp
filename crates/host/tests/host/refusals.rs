//! A refused folder gets one code per reason (decision record 0022): a drive root, the home
//! folder, a system folder, a folder or `matter.json` that does not read, a folder that is, holds
//! or sits inside another matter's (naming it), and Clauseo's own data folder.

use std::path::Path;

use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, HostError, MatterId, MatterSource, Title,
};

use crate::support::{Dir, Running, wait};
#[cfg(unix)]
use crate::write;

/// Imports `folder` as `name`.
fn import(host: &Running, folder: &Path) -> Result<CommandOutcome, HostError> {
    let folder = wait(
        "register_folder",
        host.handle.register_folder(folder.to_owned()),
    )
    .unwrap();
    host.dispatch(HostCommand::CreateMatter {
        name: Title::new("Smith v Jones").unwrap(),
        source: MatterSource::ExistingFolder { folder },
        client: None,
        next_date: None,
    })
}

/// Imports `folder` and expects `code`, naming no matter.
fn refused(host: &Running, folder: &Path, code: ErrorCode) -> HostError {
    let error = import(host, folder).expect_err("the import was refused");
    assert_eq!(error.code(), code, "{}: {error}", folder.display());
    error
}

fn imported(host: &Running, folder: &Path) -> MatterId {
    match import(host, folder) {
        Ok(CommandOutcome::MatterCreated { matter }) => matter,
        other => panic!("{}: {other:?}", folder.display()),
    }
}

#[test]
fn a_drive_root_is_folder_is_drive() {
    let dir = Dir::new("refuse_drive", &[]);
    let host = dir.start(0);
    let error = refused(&host, Path::new("/"), ErrorCode::MatterFolderIsDrive);
    assert_eq!(error.matter(), None);
    assert!(!error.retryable());
    host.shutdown();
}

#[test]
fn home_and_system_folders_have_codes_of_their_own() {
    let dir = Dir::new("refuse_home_system", &[]);
    let work = dir.folder("", &[]);
    let home = dir.folder("home-of-lawyer", &[]);
    let system = dir.folder("system", &[]);
    let inside_home = dir.folder("home-of-lawyer/Documents/Case", &[("a.txt", "x\n")]);
    let host = dir.start_with(0, |config| {
        config
            .with_home_folder(Some(home.clone()))
            .with_refused_folders(vec![home.clone(), system.clone()])
    });
    refused(&host, &home, ErrorCode::MatterFolderIsHome);
    refused(&host, &system, ErrorCode::MatterFolderIsSystem);
    // A folder inside the home folder can be a matter.
    imported(&host, &inside_home);
    // The folder holding the home folder (and now a matter below it) is refused for the home
    // folder it holds, not for the matter: the place check comes first.
    refused(&host, &work, ErrorCode::MatterFolderIsHome);
    host.shutdown();

    // Without a home folder known, the same listed folder is a system folder.
    let host = dir.start_with(1, |config| {
        config
            .with_home_folder(None)
            .with_refused_folders(vec![home.clone()])
    });
    refused(&host, &home, ErrorCode::MatterFolderIsSystem);
    host.shutdown();
}

#[test]
fn a_matter_json_that_does_not_read_is_folder_unreadable() {
    let dir = Dir::new("refuse_unreadable_json", &[]);
    let folder = dir.folder("Damaged", &[(".clauseo/matter.json", "not json")]);
    let host = dir.start(0);
    refused(&host, &folder, ErrorCode::MatterFolderUnreadable);
    // Nothing was written: the marker is as it was.
    assert_eq!(
        std::fs::read_to_string(folder.join(".clauseo").join("matter.json")).unwrap(),
        "not json"
    );
    host.shutdown();
}

#[cfg(unix)]
#[test]
fn a_folder_clauseo_may_not_read_is_folder_unreadable() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = Dir::new("refuse_unreadable_folder", &[]);
    let folder = dir.folder("Locked", &[("a.txt", "x\n")]);
    let host = dir.start(0);
    write::set_mode(&folder, 0o000);
    // A process that reads through permissions (root) would import it; the code is the
    // non-root path, which is how Clauseo runs.
    let reads_anyway = std::fs::read_dir(&folder).is_ok();
    let result = import(&host, &folder);
    write::set_mode(&folder, 0o755);
    if reads_anyway {
        assert!(result.is_ok(), "{result:?}");
    } else {
        let error = result.expect_err("refused");
        assert_eq!(error.code(), ErrorCode::MatterFolderUnreadable, "{error}");
    }
    assert_eq!(
        std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o755
    );
    host.shutdown();
}

#[test]
fn a_matters_folder_or_one_nested_with_it_is_folder_is_matter_naming_it() {
    let dir = Dir::new("refuse_is_matter", &[]);
    let folder = dir.folder("Smith v Jones", &[("a.txt", "x\n")]);
    let inside = dir.folder("Smith v Jones/Pleadings", &[("b.txt", "y\n")]);
    let host = dir.start(0);
    let matter = imported(&host, &folder);
    for (case, path) in [
        ("the same folder", folder.clone()),
        ("a folder inside it", inside),
        ("the folder holding it", folder.parent().unwrap().to_owned()),
    ] {
        let error = refused(&host, &path, ErrorCode::MatterFolderIsMatter);
        assert_eq!(error.matter(), Some(matter), "{case}");
    }
    // A new folder inside the matter's folder too.
    let parent = wait(
        "register_folder",
        host.handle.register_folder(folder.clone()),
    )
    .unwrap();
    let error = host.refused(
        HostCommand::CreateMatter {
            name: Title::new("Appeal").unwrap(),
            source: MatterSource::NewFolder { folder: parent },
            client: None,
            next_date: None,
        },
        ErrorCode::MatterFolderIsMatter,
    );
    assert_eq!(error.matter(), Some(matter));
    assert!(!folder.join("Appeal").exists(), "nothing was made");
    host.shutdown();
}

#[test]
fn clauseos_data_folder_is_folder_in_matters_root() {
    let dir = Dir::new("refuse_matters_root", &[]);
    let host = dir.start(0);
    // The data folder itself, and the matters' history inside it.
    refused(&host, dir.data(), ErrorCode::MatterFolderInMattersRoot);
    refused(
        &host,
        &dir.data().join("matters"),
        ErrorCode::MatterFolderInMattersRoot,
    );
    host.shutdown();
}

#[test]
fn a_missing_folder_is_still_folder_missing() {
    let dir = Dir::new("refuse_missing", &[]);
    let host = dir.start(0);
    refused(
        &host,
        &dir.data().join("work").join("gone"),
        ErrorCode::MatterFolderMissing,
    );
    host.shutdown();
}

#[test]
fn a_marker_naming_a_matter_this_install_never_had_names_no_matter() {
    let dir = Dir::new("refuse_foreign_marker", &[]);
    let foreign = MatterId::new(crate::support::foreign_uuid(42)).unwrap();
    let marker = format!(r#"{{"version":1,"matter_id":"{foreign}","created_at":0}}"#);
    let folder = dir.folder("Copied", &[(".clauseo/matter.json", marker.as_str())]);
    let host = dir.start(0);
    let error = refused(&host, &folder, ErrorCode::MatterFolderIsMatter);
    assert_eq!(error.matter(), None, "nothing on the list to open: {error}");
    assert!(
        host.matters()
            .iter()
            .all(|summary| summary.matter != foreign)
    );
    host.shutdown();
}

/// "New matter" on a folder that is already this install's matter, picked by another spelling
/// of its path (as `Z:\n1` and `\\server\share\n1` on Windows): the import is refused with
/// `matter.folder_is_matter` naming that matter, and nothing is made: no second matter on the
/// list, no second history, the folder's `matter.json` unchanged.
#[cfg(unix)]
#[test]
fn a_matters_folder_by_another_spelling_is_folder_is_matter_and_makes_nothing() {
    let dir = Dir::new("refuse_is_matter_spelled", &[]);
    let folder = dir.folder("Smith v Jones", &[("a.txt", "x\n")]);
    let parent = folder.parent().unwrap().to_owned();
    let link = parent.join("Alias");
    write::symlink(&folder, &link);
    let host = dir.start(0);
    let matter = imported(&host, &folder);
    let marker = std::fs::read(folder.join(".clauseo/matter.json")).unwrap();
    let histories = || {
        std::fs::read_dir(dir.data().join("matters"))
            .unwrap()
            .count()
    };
    let made = histories();
    let listed = host.matters().len();
    for (case, path) in [
        ("through a symlink", link),
        ("with `..`", folder.join("..").join("Smith v Jones")),
        ("with `.`", parent.join(".").join("Smith v Jones").join(".")),
    ] {
        let error = refused(&host, &path, ErrorCode::MatterFolderIsMatter);
        assert_eq!(error.matter(), Some(matter), "{case}: {error}");
        assert_eq!(host.matters().len(), listed, "{case}: no second matter");
        assert_eq!(histories(), made, "{case}: no second history");
        assert_eq!(
            std::fs::read(folder.join(".clauseo/matter.json")).unwrap(),
            marker,
            "{case}: matter.json unchanged"
        );
    }
    host.shutdown();
}
