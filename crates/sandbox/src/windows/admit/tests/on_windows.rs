//! The gate with the real folder check, on CI's Windows job.

use super::*;
use crate::test_support::remove_dir;
use crate::windows::folder_support;
use crate::windows::test_dacl::protect_with;
use crate::windows::tests::admin_share_path;

/// `OWNER RIGHTS` with Modify only: the owner loses `WRITE_DAC`.
const OWNER_RIGHTS_MODIFY: &str = "D:P(A;OICI;0x1301bf;;;OW)";

/// Asserts that `policy` is refused for `kind` with `want`, and that the ledger is as it was.
fn assert_refused(w: &World, policy: &Policy, kind: PathKind, want: FolderRefusal) {
    let (mut ledger, before) = ledger_with_an_entry(w);
    let groups = FakeFolders::default();
    let admitted = admit(policy, &w.at("data"), &Neighbours::default(), &groups);
    let Err(Error::FolderUnsupported { path_kind, support }) = admitted else {
        panic!("not refused: {admitted:?}");
    };
    assert_eq!((path_kind, support), (kind, want));
    // The launch's own steps refuse at the gate too, before a plan or an entry.
    let refused = prepare(
        policy,
        &w.at("data"),
        &[],
        folder_support,
        &groups,
        &mut ledger,
    );
    assert!(matches!(refused, Err(Error::FolderUnsupported { .. })));
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
}

/// Whether this machine has the admin share CI's runners have; elsewhere the test passes.
fn has_admin_share(w: &World) -> bool {
    if admin_share_path(w.dir.path()).is_dir() {
        return true;
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "CI's Windows runners have the admin shares, and this run has none"
    );
    eprintln!("skipped: this machine has no admin share for the temp folder");
    false
}

#[test]
fn a_local_world_is_admitted() {
    let w = World::new("admit-win-ok");
    let policy = w.full_policy();
    let admitted = admit(
        &policy,
        &w.at("data"),
        &Neighbours::default(),
        &FakeFolders::default(),
    )
    .unwrap();
    assert_ne!(
        plan(&admitted, &sids(), matter_one())
            .unwrap()
            .grants()
            .len(),
        0
    );
}

#[test]
fn a_matter_whose_permissions_cant_change_is_refused() {
    let w = World::new("admit-win-cant-change");
    let policy = w.full_policy();
    protect_with(&w.at("matter"), OWNER_RIGHTS_MODIFY).unwrap();
    let want = FolderRefusal::Support(FolderSupport::CantChangePermissions);
    assert_refused(&w, &policy, PathKind::Matter, want);
}

#[test]
fn a_notes_folder_whose_permissions_cant_change_is_refused() {
    let w = World::new("admit-win-notes-cant-change");
    let policy = w.full_policy();
    protect_with(&w.at(NOTES), OWNER_RIGHTS_MODIFY).unwrap();
    let want = FolderRefusal::Support(FolderSupport::CantChangePermissions);
    assert_refused(&w, &policy, PathKind::Notes, want);
}

#[test]
fn a_matter_on_a_share_is_refused() {
    let w = World::new("admit-win-network-matter");
    if !has_admin_share(&w) {
        return;
    }
    let policy = Policy::builder(admin_share_path(&w.at("matter")), w.at(STATE))
        .home(w.at("home"))
        .build()
        .unwrap();
    let want = FolderRefusal::Support(FolderSupport::Network);
    assert_refused(&w, &policy, PathKind::Matter, want);
}

#[test]
fn a_state_folder_on_a_share_refuses_the_whole_launch() {
    // The matter is local and fine; the state folder alone, through `\\localhost\C$`, is
    // enough to refuse.
    let w = World::new("admit-win-network-state");
    if !has_admin_share(&w) {
        return;
    }
    let policy = Policy::builder(w.at("matter"), admin_share_path(&w.at(STATE)))
        .home(w.at("home"))
        .build()
        .unwrap();
    assert_eq!(folder_support(policy.matter()).unwrap(), FolderSupport::Ok);
    let want = FolderRefusal::Support(FolderSupport::Network);
    assert_refused(&w, &policy, PathKind::State, want);
}

#[test]
fn a_missing_folder_is_refused() {
    let w = World::new("admit-win-missing");
    let policy = w.full_policy();
    remove_dir(&w.at(NOTES));
    let want = FolderRefusal::CheckFailed(io::ErrorKind::NotFound);
    assert_refused(&w, &policy, PathKind::Notes, want);
}

#[test]
fn a_matter_holding_the_data_folder_is_refused_in_any_letter_case() {
    // NTFS compares names without case: `MATTER\CLAUSEO` is `matter\Clauseo`.
    let w = World::new("admit-win-data-case");
    w.dir.dir("matter/Clauseo");
    let policy = w.builder().build().unwrap();
    let shouted = PathBuf::from(w.at("matter/Clauseo").to_string_lossy().to_uppercase());
    let refused = admit(
        &policy,
        &shouted,
        &Neighbours::default(),
        &FakeFolders::default(),
    );
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(path_kind, PathKind::Matter);
    assert_eq!(support, FolderRefusal::OverlapsDataFolder);
}

#[test]
fn agents_as_a_junction_like_link_refuses_on_windows() {
    // The real check: `<data>\agents` a directory link (a name-surrogate reparse point, as a
    // junction is), read through `symlink_metadata`'s `FILE_FLAG_OPEN_REPARSE_POINT` open.
    let w = World::new("admit-win-agents-link");
    w.dir.dir("elsewhere/agents");
    w.dir.dir("data2");
    let Ok(_link) = w.dir.symlink_dir("data2/agents", &w.at("elsewhere/agents")) else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's runners may make links, and this can't"
        );
        eprintln!("skipped: no right to make a link");
        return;
    };
    let policy = w.builder().build().unwrap();
    let refused = admit(
        &policy,
        &w.at("data2"),
        &Neighbours::default(),
        &FakeFolders::default(),
    );
    let Err(Error::FolderUnsupported { support, .. }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(support, FolderRefusal::ReparsePoint);
}

#[test]
fn a_claude_store_file_that_is_a_link_refuses_on_windows() {
    // A directory link at `.credentials.json`: a reparse point where the store's name is read
    // unresolved, so an entry would follow it.
    let w = World::new("admit-win-store-link");
    w.dir.dir("elsewhere/target");
    let name = format!("{STORE}/.credentials.json");
    let Ok(_link) = w.dir.symlink_dir(&name, &w.at("elsewhere/target")) else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's runners may make links, and this can't"
        );
        eprintln!("skipped: no right to make a link");
        return;
    };
    let policy = w.builder().claude_store(w.at(STORE)).build().unwrap();
    let refused = admit(
        &policy,
        &w.at("data"),
        &Neighbours::default(),
        &FakeFolders::default(),
    );
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(
        (path_kind, support),
        (PathKind::ClaudeStoreFile, FolderRefusal::ReparsePoint)
    );
}

#[test]
fn a_neighbour_under_a_junction_is_fenced_at_its_target_and_one_that_is_a_junction_refuses() {
    use crate::windows::acl_plan::Sid;
    use crate::windows::fence::{MatterFolders, RegisteredMatter};
    use crate::windows::test_junction::junction;
    use crate::windows::test_world::OTHER_POOL_SIDS;

    let w = World::new("admit-win-fence-junction");
    w.dir.dir("elsewhere/B");
    // A directory junction needs no admin rights, unlike a symbolic link.
    junction(&w.at("Linked"), &w.at("elsewhere")).unwrap();
    let neighbour = |root: PathBuf| Neighbours {
        registered: vec![RegisteredMatter {
            id: matter_two(),
            folders: MatterFolders {
                root,
                state: w.at("other/m3"),
                notes: None,
            },
            pool_user: Sid::parse(OTHER_POOL_SIDS[0]).unwrap(),
            fenced: false,
        }],
        reach: Vec::new(),
    };
    let policy = w.builder().build().unwrap();
    // Under the junction: the neighbour's own policy accepts the root, so the fence lands on the
    // target and this matter's launch passes.
    let under = w.at("Linked/B");
    let own = Policy::builder(&under, w.at("other/m3"))
        .home(w.at("home"))
        .build();
    assert!(own.is_ok(), "{own:?}");
    let admitted = admit(
        &policy,
        &w.at("data"),
        &neighbour(under),
        &FakeFolders::default(),
    )
    .unwrap();
    let paths: Vec<&Path> = admitted.fences()[0]
        .folders
        .iter()
        .map(|(p, _)| p.as_path())
        .collect();
    assert_eq!(paths, [w.canonical("elsewhere/B"), w.canonical("other/m3")]);
    // The root itself a junction: its own policy refuses it, and so does the neighbour check.
    let own = Policy::builder(w.at("Linked"), w.at("other/m3"))
        .home(w.at("home"))
        .build();
    assert!(own.is_err(), "{own:?}");
    let refused = admit(
        &policy,
        &w.at("data"),
        &neighbour(w.at("Linked")),
        &FakeFolders::default(),
    );
    let Err(Error::NeighbourUnfenceable { support, .. }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(support, FolderRefusal::ReparsePoint);
}
