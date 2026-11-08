//! `HostQuery::FolderSupport` through the host, with the folder check's answer fixed
//! (`HostConfig::with_folder_support`, `with_folder_check_failed`) so it runs on every OS: it
//! answers the values a matter's row shows for the place a `CreateMatter` with the same source
//! would use, refuses what that create refuses before its matter is made with the same codes
//! (and then never checks), and refuses a ref the host never gave. Its deadline is in
//! `actor/query/folder_support/tests.rs`.

use std::path::Path;

use clauseo_protocol::{
    ErrorCode, FolderRefId, FolderSupportKind, HostError, HostQuery, MatterSource, QueryResult,
};

use crate::support::{Dir, Running, wait};

/// `FolderSupport`'s answer for `source`, or the query's error.
fn asked(host: &Running, source: MatterSource) -> Result<Option<FolderSupportKind>, HostError> {
    match host.query(HostQuery::FolderSupport { source })? {
        QueryResult::FolderSupport(picked) => Ok(picked.support),
        other => panic!("FolderSupport answered {other:?}"),
    }
}

/// Registers `folder` as the picker would.
fn register(host: &Running, folder: &Path) -> FolderRefId {
    wait(
        "register_folder",
        host.handle.register_folder(folder.to_owned()),
    )
    .unwrap()
}

/// A host whose every check answers `network`: an answer that shows unless a refusal comes
/// first.
fn network(
    dir: &Dir,
    adjust: impl FnOnce(clauseo_host::HostConfig) -> clauseo_host::HostConfig,
) -> Running {
    dir.start_with(0, |config| {
        adjust(config.with_folder_support(Some(FolderSupportKind::Network)))
    })
}

/// The query answers the check as a row would show it: `network`, `check_failed` for a
/// failed check, nothing for a fine folder. No matter is made.
#[test]
fn a_picked_folder_answers_as_the_row_would() {
    type Configure = fn(clauseo_host::HostConfig) -> clauseo_host::HostConfig;
    let cases: [(&str, Configure, _); 3] = [
        (
            "folder_query_network",
            |config| config.with_folder_support(Some(FolderSupportKind::Network)),
            Some(FolderSupportKind::Network),
        ),
        (
            "folder_query_failed",
            clauseo_host::HostConfig::with_folder_check_failed,
            Some(FolderSupportKind::CheckFailed),
        ),
        (
            "folder_query_fine",
            |config| config.with_folder_support(None),
            None,
        ),
    ];
    for (test, config, shown) in cases {
        let dir = Dir::new(test, &[]);
        let folder = dir.folder("picked", &[]);
        let host = dir.start_with(0, config);
        let folder = register(&host, &folder);
        let before = host.matters().len();
        for source in [
            MatterSource::ExistingFolder { folder },
            MatterSource::NewFolder { folder },
        ] {
            assert_eq!(asked(&host, source).unwrap(), shown, "{test} {source:?}");
        }
        assert_eq!(
            asked(&host, MatterSource::DefaultLocation).unwrap(),
            shown,
            "{test}"
        );
        assert_eq!(host.matters().len(), before, "{test}");
        host.shutdown();
    }
}

/// An existing folder create refuses (the data folder or one inside it, the home folder, a
/// drive root, another matter's folder) is refused with create's code and never checked: the
/// fixed `network` answer never shows.
#[test]
fn an_existing_folder_create_refuses_is_refused_with_its_code_and_never_checked() {
    let dir = Dir::new("folder_query_refused", &[]);
    let home = dir.folder("home-of-lawyer", &[]);
    let matter = dir.folder("home-of-lawyer/Case", &[("a.txt", "x\n")]);
    let host = network(&dir, |config| {
        config
            .with_home_folder(Some(home.clone()))
            .with_refused_folders(vec![home.clone()])
    });
    host.create_matter("Case", &matter);
    let inside_data = dir.data().join("y");
    crate::write::create_dir_all(&inside_data);
    for (folder, code) in [
        (
            dir.data().to_path_buf(),
            ErrorCode::MatterFolderInMattersRoot,
        ),
        // Inside the data folder: import's protected-tree check, after the registry.
        (inside_data, ErrorCode::MatterFolderInMattersRoot),
        (home, ErrorCode::MatterFolderIsHome),
        (Path::new("/").to_path_buf(), ErrorCode::MatterFolderIsDrive),
        (matter, ErrorCode::MatterFolderIsMatter),
    ] {
        let source = MatterSource::ExistingFolder {
            folder: register(&host, &folder),
        };
        let error = asked(&host, source).unwrap_err();
        assert_eq!(error.code(), code, "{}: {error:?}", folder.display());
    }
    host.shutdown();
}

/// A new folder's parent gets only what create checks of a parent, which is nothing: the home
/// folder and a drive root are parents create takes, so the query checks them.
#[test]
fn a_new_folders_parent_is_checked_as_create_takes_it() {
    let dir = Dir::new("folder_query_parent", &[]);
    let home = dir.folder("home-of-lawyer", &[]);
    let host = network(&dir, |config| {
        config
            .with_home_folder(Some(home.clone()))
            .with_refused_folders(vec![home.clone()])
    });
    for parent in [home.as_path(), Path::new("/")] {
        let source = MatterSource::NewFolder {
            folder: register(&host, parent),
        };
        assert_eq!(
            asked(&host, source).unwrap(),
            Some(FolderSupportKind::Network),
            "{}",
            parent.display()
        );
    }
    host.shutdown();
}

/// A new folder whose parent is inside the data folder (the data folder itself, its matters
/// folder, a folder under it) is refused with create's code and never checked: the fixed
/// `network` answer never shows. The home folder, which holds the data folder, is accepted as a
/// parent, since create makes a folder in it that is outside the data folder.
#[test]
fn a_new_folder_inside_the_data_folder_is_refused_and_the_home_holding_it_is_not() {
    let dir = Dir::new("folder_query_new_data", &[]);
    let inside = dir.data().join("x");
    crate::write::create_dir_all(&inside);
    let host = network(&dir, |config| {
        config.with_home_folder(Some(dir.home().to_owned()))
    });
    for parent in [dir.data().to_path_buf(), dir.data().join("matters"), inside] {
        let source = MatterSource::NewFolder {
            folder: register(&host, &parent),
        };
        let error = asked(&host, source).unwrap_err();
        assert_eq!(
            error.code(),
            ErrorCode::MatterFolderInMattersRoot,
            "{}: {error:?}",
            parent.display()
        );
    }
    let source = MatterSource::NewFolder {
        folder: register(&host, dir.home()),
    };
    assert_eq!(
        asked(&host, source).unwrap(),
        Some(FolderSupportKind::Network),
        "the home folder"
    );
    host.shutdown();
}

/// A ref the host never gave is `command.invalid`, as for the commands that take one.
#[test]
fn an_unknown_folder_ref_is_refused() {
    let dir = Dir::new("folder_query_unknown", &[]);
    let host = network(&dir, |config| config);
    let folder = FolderRefId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_00ff,
    ))
    .unwrap();
    for source in [
        MatterSource::ExistingFolder { folder },
        MatterSource::NewFolder { folder },
    ] {
        let error = asked(&host, source).unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error:?}");
    }
    host.shutdown();
}

/// Windows only: the picked-folder query refuses a folder too deep for Windows to start programs
/// in with the code create gives, so the dialog never shows a kind for it.
#[cfg(windows)]
#[test]
fn a_folder_too_deep_for_windows_is_refused_in_the_query() {
    let dir = Dir::new("folder_query_too_deep", &[]);
    let name = vec!["d".repeat(40); 6].join("/");
    let deep = dir.folder(&name, &[]);
    let host = network(&dir, |config| config);
    let source = MatterSource::ExistingFolder {
        folder: register(&host, &deep),
    };
    let error = asked(&host, source).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderTooDeep, "{error:?}");
    host.shutdown();
}
