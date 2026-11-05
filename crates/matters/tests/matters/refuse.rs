//! Folders a new matter may not be made of: another matter's folder, and the matters root.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Error, Imported, Matter, MissingReason, NewMatter, Refusal, Settings, scan};
use clauseo_protocol::ErrorCode;

use crate::fixture::{Fixture, at, matter_id};
use crate::support;

/// The refusal of both `scan` and `import` (as matter 2) of `folder`; asserts they agree and
/// that the import left no history behind.
fn refusals(fx: &Fixture, settings: &Settings, folder: &Path) -> Refusal {
    let cancel = AtomicBool::new(false);
    let scanned = scan(folder, &[], settings, &mut |_| {}, &cancel).unwrap_err();
    let new = NewMatter {
        id: matter_id(2),
        folder: folder.to_owned(),
        ..fx.new_matter()
    };
    let imported = Matter::import(settings.clone(), new, &[], &mut |_| {}, &cancel).unwrap_err();
    assert!(
        !settings
            .matters_root
            .join(matter_id(2).to_string())
            .exists()
    );
    let reason = |err: Error| {
        assert_eq!(err.code(), ErrorCode::MatterFolderRefused, "{err:?}");
        match err {
            Error::FolderRefused { reason, .. } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    };
    let from_scan = reason(scanned);
    assert_eq!(reason(imported), from_scan);
    from_scan
}

#[test]
fn import_refuses_folder_of_another_matter() {
    let fx = Fixture::new("refuse-other-matter");
    fx.write("a.txt", "a");
    drop(fx.import());
    let json = fx.read(".clauseo/matter.json");

    let reason = refusals(&fx, &fx.settings(), &fx.folder);
    assert_eq!(reason, Refusal::AlreadyMatter(matter_id(1)));
    assert_eq!(fx.read(".clauseo/matter.json"), json);
    let mut matter = fx.open(&fx.folder).unwrap();
    matter.before_turn(&crate::fixture::meta(2)).unwrap();

    // A matter.json that does not read may still be a matter's: refused, left as it is.
    let garbage = fx.dir.path().join("garbage");
    support::write(garbage.join(".clauseo/matter.json"), "{not json");
    support::write(garbage.join("b.txt"), "b");
    let reason = refusals(&fx, &fx.settings(), &garbage);
    assert_eq!(reason, Refusal::UnreadableMatterJson);
    let bytes = std::fs::read(garbage.join(".clauseo/matter.json")).unwrap();
    assert_eq!(bytes, b"{not json");
}

#[test]
fn import_refuses_matters_root_nesting() {
    let fx = Fixture::new("refuse-root");
    let settings = fx.settings();
    let root = fx.root();
    support::write(root.join("notes.txt"), "the root itself");
    support::write(root.join("inside/a.txt"), "inside the root");
    support::write(fx.dir.path().join("beside.txt"), "holds the root");

    for folder in [root.clone(), root.join("inside"), fx.dir.path().to_owned()] {
        let reason = refusals(&fx, &settings, &folder);
        assert_eq!(reason, Refusal::MattersRoot, "{}", folder.display());
        assert!(!folder.join(".clauseo").exists(), "{}", folder.display());
    }
    assert!(!root.join("inside/.clauseo").exists());
}

/// A plain file is refused before anything is written, by scan and import alike.
#[test]
fn import_refuses_plain_file_up_front() {
    let fx = Fixture::new("refuse-file");
    let file = fx.dir.path().join("notes.txt");
    support::write(&file, "a file, not a folder");
    let settings = fx.settings();
    let cancel = AtomicBool::new(false);
    let scanned = scan(&file, &[], &settings, &mut |_| {}, &cancel).unwrap_err();
    let new = NewMatter {
        id: matter_id(2),
        folder: file.clone(),
        ..fx.new_matter()
    };
    let imported = Matter::import(settings.clone(), new, &[], &mut |_| {}, &cancel).unwrap_err();
    for err in [scanned, imported] {
        assert!(
            matches!(
                err,
                Error::FolderMissing {
                    reason: MissingReason::NoFolder,
                    ..
                }
            ),
            "{err:?}"
        );
    }
    assert!(
        !settings.matters_root.exists(),
        "nothing written, not even the matters root"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"a file, not a folder");
}

/// The settings of a test whose data folder `data` is a refused tree: the matters root sits
/// inside it, as the host's does.
fn tree_settings(fx: &Fixture, data: &Path) -> Settings {
    let mut settings = fx.settings();
    settings.matters_root = data.join("matters");
    settings.refused_trees = vec![data.to_owned()];
    settings
}

/// The refusal of `Matter::locate` of `folder`, which must refuse.
fn located_refusal(settings: &Settings, folder: &Path) -> Refusal {
    let err = Matter::locate(settings.clone(), matter_id(1), folder, &[]).unwrap_err();
    refusal_of(err)
}

/// The refusal of `Matter::relink` of `folder`, which must refuse.
fn relinked_refusal(settings: &Settings, folder: &Path) -> Refusal {
    let err = Matter::relink(settings.clone(), matter_id(1), folder, at(0), &[]).unwrap_err();
    refusal_of(err)
}

fn refusal_of(err: Error) -> Refusal {
    assert_eq!(err.code(), ErrorCode::MatterFolderRefused, "{err:?}");
    match err {
        Error::FolderRefused { reason, .. } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A folder inside a refused tree cannot be a matter: import (and scan), locate and relink refuse
/// it with the tree's entry, and write nothing.
#[test]
fn refused_tree_refuses_folders_inside_it() {
    let fx = Fixture::new("refuse-tree");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let agents = data.join("agents");
    let deep = data.join("x").join("y");
    support::write(agents.join("config.toml"), "model = \"a\"");
    support::write(deep.join("a.txt"), "a");

    for folder in [&agents, &deep] {
        let listed = Refusal::Listed(data.clone());
        assert_eq!(
            refusals(&fx, &settings, folder),
            listed,
            "{}",
            folder.display()
        );
        assert_eq!(
            located_refusal(&settings, folder),
            listed,
            "{}",
            folder.display()
        );
        assert_eq!(
            relinked_refusal(&settings, folder),
            listed,
            "{}",
            folder.display()
        );
        assert!(!folder.join(".clauseo").exists(), "{}", folder.display());
    }
}

/// The data folder itself, and a folder holding it, are refused too. Here the matters root is
/// outside the tree (the fixture's `root`), so the reason is the tree's.
#[test]
fn refused_tree_refuses_the_tree_and_its_parent() {
    let fx = Fixture::new("refuse-tree-parent");
    let host = fx.dir.path().join("host");
    let data = host.join("data");
    let mut settings = fx.settings();
    settings.refused_trees = vec![data.clone()];
    support::write(data.join("notes.txt"), "the data folder");

    for folder in [data.clone(), host] {
        let listed = Refusal::Listed(data.clone());
        assert_eq!(
            refusals(&fx, &settings, &folder),
            listed,
            "{}",
            folder.display()
        );
        assert_eq!(
            located_refusal(&settings, &folder),
            listed,
            "{}",
            folder.display()
        );
        assert_eq!(
            relinked_refusal(&settings, &folder),
            listed,
            "{}",
            folder.display()
        );
    }
}

/// A folder beside the refused tree, even one whose name starts with the tree's, is accepted.
#[test]
fn refused_tree_accepts_folders_beside_it() {
    let fx = Fixture::new("refuse-tree-beside");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let beside = fx.dir.path().join("data-notes");
    support::write(beside.join("a.txt"), "a");

    let cancel = AtomicBool::new(false);
    scan(&beside, &[], &settings, &mut |_| {}, &cancel).unwrap();
    let new = NewMatter {
        id: matter_id(2),
        folder: beside.clone(),
        ..fx.new_matter()
    };
    let imported = Matter::import(settings, new, &[], &mut |_| {}, &cancel).unwrap();
    assert!(matches!(imported, Imported::Done { .. }));
    assert!(beside.join(".clauseo/matter.json").exists());
}

/// A folder inside a listed (non-tree) folder, the home folder say, stays importable; the listed
/// folder itself is refused as it was.
#[test]
fn listed_folder_keeps_its_rule_beside_a_refused_tree() {
    let fx = Fixture::new("refuse-listed");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let home = fx.dir.path().join("home");
    let docs = home.join("docs");
    support::write(docs.join("a.txt"), "a");
    let cancel = AtomicBool::new(false);
    let listed = std::slice::from_ref(&home);

    let new = NewMatter {
        id: matter_id(2),
        folder: home.clone(),
        ..fx.new_matter()
    };
    let refused = Matter::import(settings.clone(), new, listed, &mut |_| {}, &cancel).unwrap_err();
    assert_eq!(refusal_of(refused), Refusal::Listed(home.clone()));

    let new = NewMatter {
        id: matter_id(2),
        folder: docs.clone(),
        ..fx.new_matter()
    };
    let imported = Matter::import(settings, new, listed, &mut |_| {}, &cancel).unwrap();
    assert!(matches!(imported, Imported::Done { .. }));
    assert!(docs.join(".clauseo/matter.json").exists());
}

/// The matters root inside the refused tree (the host's layout) is still refused as the matters
/// root, and so is the tree that holds it: the matters root is checked first.
#[test]
fn refused_tree_keeps_the_matters_root_reason() {
    let fx = Fixture::new("refuse-tree-root");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let root = data.join("matters");
    support::write(root.join("notes.txt"), "the root");
    assert_eq!(refusals(&fx, &settings, &root), Refusal::MattersRoot);
    assert_eq!(located_refusal(&settings, &root), Refusal::MattersRoot);
    assert_eq!(refusals(&fx, &settings, &data), Refusal::MattersRoot);
}

/// Refused tree entries match in any case on macOS and Windows, as listed entries do.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn refused_tree_matches_in_any_case() {
    let fx = Fixture::new("refuse-tree-case");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    support::write(data.join("agents").join("config.toml"), "model = \"a\"");
    let spelled = fx.dir.path().join("DATA").join("agents");
    assert_eq!(
        refusals(&fx, &settings, &spelled),
        Refusal::Listed(data.clone())
    );
    assert_eq!(located_refusal(&settings, &spelled), Refusal::Listed(data));
}

/// A path not made yet under a symlinked folder (macOS `/var` is `/private/var`) compares with
/// the real folder: its deepest existing ancestor is canonicalised and the rest appended.
#[cfg(unix)]
#[test]
fn a_missing_path_under_a_symlink_compares_with_the_real_folder() {
    let dir = support::TestDir::new("refuse-nested-link");
    let real = dir.path().join("real");
    support::create_dir_all(&real);
    let link = dir.path().join("link");
    support::symlink(&real, &link);
    assert!(clauseo_matters::same_or_nested(
        &link.join("missing"),
        &real
    ));
    assert!(clauseo_matters::same_or_nested(
        &real,
        &link.join("missing").join("deeper")
    ));
    assert!(clauseo_matters::same_or_nested(
        &link.join("missing"),
        &real.join("missing")
    ));
    assert!(!clauseo_matters::same_or_nested(
        &link.join("missing"),
        &real.join("other")
    ));
    // A path whose missing part has no name is taken as written.
    assert!(!clauseo_matters::same_or_nested(
        &link.join("missing").join(".."),
        &real
    ));
}

/// Create makes a new folder only where import would take the folder: a new folder inside a
/// refused tree (the data folder) or the matters root is refused with the reason import gives
/// for the same path, and nothing is made (no folder, no marker, no history).
#[test]
fn create_refuses_a_new_folder_inside_a_refused_tree() {
    let fx = Fixture::new("refuse-create-tree");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let parent = data.join("x");
    support::create_dir_all(&parent);
    let root = data.join("matters");
    for (folder, want) in [
        (parent.join("Appeal"), Refusal::Listed(data.clone())),
        (data.join("Appeal"), Refusal::Listed(data.clone())),
        (root.join("Appeal"), Refusal::MattersRoot),
    ] {
        let new = NewMatter {
            folder: folder.clone(),
            ..fx.new_matter()
        };
        let err = Matter::create(settings.clone(), new).unwrap_err();
        assert_eq!(err.code(), ErrorCode::MatterFolderRefused, "{err:?}");
        let reason = match err {
            Error::FolderRefused { reason, .. } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(reason, want, "{}", folder.display());
        // The import of the same path, once it exists, is refused for the same reason.
        support::create_dir_all(&folder);
        assert_eq!(
            refusals(&fx, &settings, &folder),
            want,
            "{}",
            folder.display()
        );
        support::remove_dir_all(&folder);
        assert!(!folder.exists(), "{}", folder.display());
        assert!(
            !settings
                .matters_root
                .join(matter_id(1).to_string())
                .exists()
        );
        assert!(!root.join(matter_id(1).to_string()).exists());
    }
    assert!(!data.join("Appeal").exists());
    assert!(!parent.join("Appeal").exists());
}

/// A new folder beside the refused tree, or under a folder that holds it, is made as before.
#[test]
fn create_accepts_a_new_folder_outside_the_refused_tree() {
    let fx = Fixture::new("refuse-create-beside");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    let home = fx.dir.path().join("home");
    support::create_dir_all(&home);
    let folders = [
        home.join("Appeal"),
        fx.dir.path().join("data-notes").join("Appeal"),
    ];
    for (n, folder) in (1..).zip(folders) {
        let new = NewMatter {
            id: matter_id(n),
            folder: folder.clone(),
            ..fx.new_matter()
        };
        let matter = Matter::create(settings.clone(), new).unwrap();
        assert_eq!(matter.folder(), folder);
        assert!(
            folder.join(".clauseo/matter.json").is_file(),
            "{}",
            folder.display()
        );
    }
}

/// The protected-folder check takes a path that is not made yet: it refuses what sits inside
/// the data folder and accepts what does not, and makes nothing.
#[test]
fn check_protected_takes_a_folder_not_made_yet() {
    let fx = Fixture::new("refuse-protected");
    let data = fx.dir.path().join("data");
    let settings = tree_settings(&fx, &data);
    support::create_dir_all(data.join("x"));
    for folder in [data.join("x").join("_"), data.join("_")] {
        let err = clauseo_matters::check_protected(&folder, &settings).unwrap_err();
        assert_eq!(
            err.code(),
            ErrorCode::MatterFolderRefused,
            "{}",
            folder.display()
        );
        assert!(matches!(
            err,
            Error::FolderRefused { reason: Refusal::Listed(ref tree), .. } if *tree == data
        ));
    }
    clauseo_matters::check_protected(&fx.dir.path().join("_"), &settings).unwrap();
    clauseo_matters::check_protected(&fx.dir.path().join("data-notes").join("_"), &settings)
        .unwrap();
    assert!(!data.join("x").join("_").exists());
}
