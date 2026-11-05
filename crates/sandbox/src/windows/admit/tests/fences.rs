//! The gate's checks of the registered matters' fences.

use super::*;
use crate::windows::acl_plan::Sid;
use crate::windows::fence::{MatterFolders, RegisteredMatter};
use crate::windows::test_world::OTHER_POOL_SIDS;

/// Neighbours with one unfenced registered matter, matter two, at `root` and `state` as given.
fn unfenced(root: PathBuf, state: PathBuf) -> Neighbours {
    Neighbours {
        registered: vec![RegisteredMatter {
            id: matter_two(),
            folders: MatterFolders {
                root,
                state,
                notes: None,
            },
            pool_user: Sid::parse(OTHER_POOL_SIDS[0]).unwrap(),
            fenced: false,
        }],
        reach: Vec::new(),
    }
}

/// A folder check that answers `NotFound` for a missing path, as `folder_support` does.
fn by_existence(path: &Path) -> io::Result<FolderSupport> {
    if path.exists() {
        Ok(FolderSupport::Ok)
    } else {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

#[test]
fn fence_paths_are_resolved_once_to_their_canonical_spelling() {
    // `w.at` is the temp folder as given (`/var/...` on macOS, a link to `/private/var`).
    let w = World::new("admit-fence-spelling");
    let policy = w.builder().build().unwrap();
    let given = unfenced(w.at("other/m2"), w.at("other/m3"));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        by_existence,
        &FakeFolders::default(),
    )
    .unwrap();
    let fences = admitted.fences();
    assert_eq!(fences.len(), 1);
    let paths: Vec<&Path> = fences[0].folders.iter().map(|(p, _)| p.as_path()).collect();
    assert_eq!(paths, [w.canonical("other/m2"), w.canonical("other/m3")]);
    let root = &admitted.neighbours().registered[0].folders.root;
    assert_eq!(
        root,
        &w.canonical("other/m2"),
        "the exclusions see the same spelling"
    );
}

#[test]
fn a_missing_fence_folder_is_left_out() {
    let w = World::new("admit-fence-missing");
    let policy = w.builder().build().unwrap();
    let given = unfenced(w.at("other/not-made-yet"), w.at("other/m3"));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        by_existence,
        &FakeFolders::default(),
    )
    .unwrap();
    let kinds: Vec<PathKind> = admitted.fences()[0]
        .folders
        .iter()
        .map(|(_, k)| *k)
        .collect();
    assert_eq!(kinds, [PathKind::State]);
}

#[test]
fn a_fence_folder_that_cant_take_it_refuses_the_launch_naming_the_matter() {
    let w = World::new("admit-fence-refused");
    let policy = w.builder().build().unwrap();
    let state = w.canonical("other/m3");
    let given = unfenced(w.at("other/m2"), state.clone());
    let (ledger, before) = ledger_with_an_entry(&w);
    let check = answering(state, || Ok(FolderSupport::CantChangePermissions));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        check,
        &FakeFolders::default(),
    );
    let Err(
        error @ Error::NeighbourUnfenceable {
            matter,
            path_kind,
            support,
        },
    ) = admitted
    else {
        panic!("not refused: {admitted:?}");
    };
    assert_eq!((matter, path_kind), (matter_two(), PathKind::State));
    assert_eq!(
        support,
        FolderRefusal::Support(FolderSupport::CantChangePermissions)
    );
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    assert!(
        !error.to_string().contains(&*root.to_string_lossy()),
        "{error}"
    );
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn a_linked_fence_folder_refuses_the_launch() {
    let w = World::new("admit-fence-link");
    let policy = w.builder().build().unwrap();
    let link = w.dir.symlink("other/linked", &w.at("elsewhere"));
    let given = unfenced(link, w.at("other/m3"));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        by_existence,
        &FakeFolders::default(),
    );
    let Err(Error::NeighbourUnfenceable { support, .. }) = admitted else {
        panic!("not refused: {admitted:?}");
    };
    assert_eq!(support, FolderRefusal::ReparsePoint);
}

#[cfg(unix)]
#[test]
fn a_neighbour_its_own_gate_accepts_is_fenced_at_its_target_and_the_launch_passes() {
    // The neighbour's root sits under a linked folder (`Linked` -> `elsewhere`, as a junction
    // to another drive would): its own policy accepts it, so the fence lands on the target.
    let w = World::new("admit-fence-through-link");
    w.dir.dir("elsewhere/B");
    w.dir.symlink("Linked", &w.at("elsewhere"));
    let root = w.at("Linked/B");
    let own = Policy::builder(&root, w.at("other/m3"))
        .home(w.at("home"))
        .build();
    assert!(
        own.is_ok(),
        "the neighbour's own launch accepts its root: {own:?}"
    );
    let policy = w.builder().build().unwrap();
    let given = unfenced(root, w.at("other/m3"));
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        by_existence,
        &FakeFolders::default(),
    )
    .unwrap();
    let paths: Vec<&Path> = admitted.fences()[0]
        .folders
        .iter()
        .map(|(p, _)| p.as_path())
        .collect();
    assert_eq!(paths, [w.canonical("elsewhere/B"), w.canonical("other/m3")]);
    // Its own launch refuses a root that is itself a link; so does the neighbour check.
    let own = Policy::builder(w.at("Linked"), w.at("other/m3"))
        .home(w.at("home"))
        .build();
    assert!(own.is_err());
    let linked = unfenced(w.at("Linked"), w.at("other/m3"));
    let refused = admit_with(
        &policy,
        &w.at("data"),
        &linked,
        by_existence,
        &FakeFolders::default(),
    );
    assert!(
        matches!(refused, Err(Error::NeighbourUnfenceable { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_fenced_matter_is_not_checked() {
    let w = World::new("admit-fence-fenced");
    let policy = w.builder().build().unwrap();
    let mut given = unfenced(w.at("other/m2"), w.at("other/m3"));
    given.registered[0].fenced = true;
    let check = answering(w.canonical("other/m3"), || {
        Ok(FolderSupport::CantChangePermissions)
    });
    let admitted = admit_with(
        &policy,
        &w.at("data"),
        &given,
        check,
        &FakeFolders::default(),
    );
    assert_eq!(admitted.unwrap().fences(), []);
}
