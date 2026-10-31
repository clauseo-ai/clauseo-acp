//! Folders a new matter may not be made of: another matter's folder, and the matters root.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Error, Matter, MissingReason, NewMatter, Refusal, Settings, scan};
use clauseo_protocol::ErrorCode;

use crate::fixture::{Fixture, matter_id};
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
