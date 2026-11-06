//! S15 `PR4h`: in a matter's tree, a child (file or folder, at any depth) a pool user other
//! than the matter's own owns goes to the lawyer, as main's hand-back does (S15 `PR4a2`); only
//! one whose hand-back fails refuses, as `PoolOwned { owned: Child }`, naming it.

use super::fake::LAWYER;
use super::*;
use crate::windows::writer::Owned;

const CHILD: ApplyFailure = ApplyFailure::PoolOwned {
    owned: Owned::Child,
};

/// `rel` inside `w`'s canonical matter folder, made as a file (or a folder, ending in `/`).
fn in_matter(w: &World, rel: &str) -> PathBuf {
    let made = match rel.strip_suffix('/') {
        Some(folder) => w.dir.dir(&format!("matter/{folder}")),
        None => w.dir.file(&format!("matter/{rel}"), "x"),
    };
    dunce::canonicalize(made).unwrap()
}

/// Matter one and matter two (at `other/m2`, assigned the first other pool user) registered.
fn two_registered(w: &World) -> [RegisteredMatter; 2] {
    w.dir.dir("other/m2.state");
    let one = RegisteredMatter {
        id: matter_one(),
        folders: MatterFolders {
            root: w.at("matter"),
            state: w.at(STATE),
            notes: Some(w.at(NOTES)),
        },
        pool_user: sid(USER_SID),
        fenced: true,
    };
    let two = RegisteredMatter {
        id: matter_two(),
        folders: MatterFolders {
            root: w.at("other/m2"),
            state: w.at("other/m2.state"),
            notes: None,
        },
        pool_user: sid(OTHER_POOL_SIDS[0]),
        fenced: true,
    };
    [one, two]
}

#[test]
fn another_pool_users_child_goes_to_the_lawyer_at_any_depth_and_the_matters_own_stays() {
    let w = world("child-owners-handed", &[]);
    let deep = in_matter(&w, "a/b/c/moved.txt");
    let folder = in_matter(&w, "from-m2/");
    let mine = in_matter(&w, "a/mine.txt");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.owners
        .insert(deep.clone(), OTHER_POOL_SIDS[0].to_owned());
    acl.owners
        .insert(folder.clone(), OTHER_POOL_SIDS[1].to_owned());
    acl.owners.insert(mine.clone(), USER_SID.to_owned());

    let applied = apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    );

    assert!(applied.unwrap().walked.repaired >= 2);
    assert_eq!(acl.owners[&deep], LAWYER);
    assert_eq!(acl.owners[&folder], LAWYER);
    assert_eq!(
        acl.owners[&mine], USER_SID,
        "the matter's own pool user's: expected"
    );
    assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
}

#[test]
fn a_child_whose_hand_back_fails_refuses_the_launch_naming_it() {
    let w = world("child-owners-kept", &[]);
    let child = in_matter(&w, "a/moved.txt");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.owners
        .insert(child.clone(), OTHER_POOL_SIDS[0].to_owned());
    acl.keeps_owner.insert(child.clone());

    let result = apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    );

    let Err(Error::AclApply { failure, at, .. }) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!((failure, at), (CHILD, Some(child.clone())));
    assert_eq!(acl.owners[&child], OTHER_POOL_SIDS[0]);
    assert!(
        !ledger.entries().any(|e| e.state == EntryState::Applied),
        "the launch's entries rolled back"
    );
    let shown = CHILD.to_string();
    assert!(!shown.contains('\\') && !shown.contains("S-1-"), "{shown}");
}

/// A child the lawyer can't open still takes main's path: it is set aside, not refused.
#[test]
fn a_child_the_lawyer_cant_open_is_set_aside_not_refused() {
    let w = world("child-owners-shut", &[]);
    let shut = in_matter(&w, "a/shut.txt");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.shut = vec![shut.clone()];
    let mut around = around(&w);

    apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around,
    )
    .unwrap();

    assert_eq!(around.set_aside, vec![shut]);
}

/// A sibling's walk is outside every matter's tree: its children keep their owners.
#[test]
fn a_walk_outside_a_matters_tree_hands_nothing_back() {
    let w = world("child-owners-sibling", &[]);
    let child = dunce::canonicalize(w.dir.file("sib1/theirs.txt", "x")).unwrap();
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.owners
        .insert(child.clone(), OTHER_POOL_SIDS[0].to_owned());

    let plan = &mut launch_plan(&w, &["sib1"]);
    apply(&mut ledger, plan, &mut acl, &mut around(&w)).unwrap();

    assert!(
        acl.walks
            .iter()
            .any(|(root, ..)| *root == w.canonical("sib1"))
    );
    assert_eq!(acl.owners[&child], OTHER_POOL_SIDS[0]);
}

/// A fence walk of matter two's tree holds it to matter two's pool user: matter one's user's
/// child there goes to the lawyer, matter two's own stays.
#[test]
fn a_fence_walk_hands_back_what_another_matters_user_owns_in_a_registered_matter() {
    let w = world("child-owners-fence", &[]);
    let one_s = dunce::canonicalize(w.dir.file("other/m2/one.txt", "x")).unwrap();
    let own = dunce::canonicalize(w.dir.file("other/m2/own.txt", "x")).unwrap();
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.owners.insert(one_s.clone(), USER_SID.to_owned());
    acl.owners
        .insert(own.clone(), OTHER_POOL_SIDS[0].to_owned());
    let pool = [
        sid(USER_SID),
        sid(OTHER_POOL_SIDS[0]),
        sid(OTHER_POOL_SIDS[1]),
    ];

    let registered = two_registered(&w);
    fence_registered(&mut ledger, &registered, &pool, &mut acl, &mut around(&w)).unwrap();

    assert_eq!(acl.owners[&one_s], LAWYER);
    assert_eq!(acl.owners[&own], OTHER_POOL_SIDS[0]);
}

/// The repair when the agent exits holds the trees to the matter's ledger entries, its pool
/// user the assigned one.
#[test]
fn the_repair_after_a_session_hands_back_a_child_another_pool_user_owns() {
    let w = world("child-owners-repair", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let moved = in_matter(&w, "moved-in/");
    let mine = in_matter(&w, "made.txt");
    acl.owners
        .insert(moved.clone(), OTHER_POOL_SIDS[1].to_owned());
    acl.owners.insert(mine.clone(), USER_SID.to_owned());

    repair_after_session(&ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();

    assert_eq!(acl.owners[&moved], LAWYER);
    assert_eq!(acl.owners[&mine], USER_SID);
}
