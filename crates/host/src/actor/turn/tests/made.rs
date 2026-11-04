//! A "New matter" whose record fails after its worker made the matter on disk (stage B saw it
//! with `store.io`): the folder's `matter.json` and the matter's history are taken back before
//! the command is answered with the store's own error, so a retry on the same folder makes the
//! matter instead of finding the folder "already a matter".

use std::path::Path;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, ErrorCode, FolderRefId, HostCommand, MatterSource, Title,
};

use super::rig::Rig;
use crate::error::Error;
use crate::pipeline::tests::support::TempDir;

/// The folder ref of `folder`, as `register_folder` mints one.
fn folder_ref(rig: &mut Rig, folder: &Path) -> FolderRefId {
    let now = rig.actor.now().wall;
    let id: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(id, folder.to_owned());
    id
}

fn create(source: MatterSource) -> HostCommand {
    HostCommand::CreateMatter {
        name: Title::new("Smith v Jones").unwrap(),
        source,
        client: None,
        next_date: None,
    }
}

/// The matters' histories under the data folder.
fn histories(rig: &Rig) -> usize {
    std::fs::read_dir(rig.actor.effective.data_dir().join("matters"))
        .unwrap()
        .count()
}

/// The store refuses the next commit that creates a matter: `fail` gives it a second receipt for
/// the command of a thread made first.
fn store_refuses_the_next_matter(rig: &mut Rig) {
    let _thread = rig.thread();
    rig.fail(1, |event| matches!(event, DomainEvent::MatterCreated(_)));
}

#[test]
fn an_import_whose_record_fails_leaves_nothing_and_imports_again() {
    let mut rig = Rig::start("made-import", "");
    let papers = TempDir::new("made-import-papers");
    let folder = papers.path().join("Smith v Jones");
    crate::write::create_dir_all(&folder.join(".clauseo")).unwrap();
    crate::write::write_file(&folder.join(".clauseo/notes.txt"), b"kept").unwrap();
    crate::write::write_file(&folder.join("brief.txt"), b"Clause 4\n").unwrap();
    let (listed, made) = (rig.actor.matters.len(), histories(&rig));
    let source = MatterSource::ExistingFolder {
        folder: folder_ref(&mut rig, &folder),
    };

    store_refuses_the_next_matter(&mut rig);
    let error = rig.dispatch(10, create(source)).unwrap_err();
    // The store's own error, as the fault makes it (a duplicate receipt, `internal`).
    assert!(matches!(error, Error::Store(_)), "{error:?}");
    assert_eq!(error.code(), ErrorCode::Internal, "{error}");
    assert!(
        !folder.join(".clauseo/matter.json").exists(),
        "no marker left"
    );
    assert_eq!(
        std::fs::read(folder.join(".clauseo/notes.txt")).unwrap(),
        b"kept",
        "the .clauseo that was there stays"
    );
    assert_eq!(histories(&rig), made, "no history left");
    assert_eq!(rig.actor.matters.len(), listed);
    assert!(rig.actor.made.is_empty() && rig.actor.claims.is_empty());

    let answer = rig.dispatch(11, create(source)).unwrap();
    let CommandOutcome::MatterCreated { matter } = answer else {
        panic!("{answer:?}");
    };
    assert!(rig.actor.matters[&matter].worker.is_some());
    assert!(folder.join(".clauseo/matter.json").exists());
    assert_eq!(histories(&rig), made + 1);
    rig.finish();
}

#[test]
fn a_new_folder_whose_record_fails_is_removed_and_made_again() {
    let mut rig = Rig::start("made-create", "");
    let papers = TempDir::new("made-create-papers");
    let made = histories(&rig);
    let source = MatterSource::NewFolder {
        folder: folder_ref(&mut rig, papers.path()),
    };

    store_refuses_the_next_matter(&mut rig);
    let error = rig.dispatch(10, create(source)).unwrap_err();
    // The store's own error, as the fault makes it (a duplicate receipt, `internal`).
    assert!(matches!(error, Error::Store(_)), "{error:?}");
    assert_eq!(error.code(), ErrorCode::Internal, "{error}");
    assert!(
        !papers.path().join("Smith v Jones").exists(),
        "the new folder is gone"
    );
    assert_eq!(histories(&rig), made, "no history left");

    let answer = rig.dispatch(11, create(source)).unwrap();
    assert!(
        matches!(answer, CommandOutcome::MatterCreated { .. }),
        "{answer:?}"
    );
    assert!(
        papers
            .path()
            .join("Smith v Jones/.clauseo/matter.json")
            .exists()
    );
    rig.finish();
}
