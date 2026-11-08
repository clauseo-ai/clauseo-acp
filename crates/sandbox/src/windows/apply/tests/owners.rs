//! S15 `PR4a2`: a protected item a pool user owns goes to the lawyer before its deny is
//! written, since an owner gets `WRITE_DAC` before any deny is read; so does what a pool user
//! owns inside a protected folder newly denied. The agent's other files stay its own.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::fake::LAWYER;
use super::*;
use crate::windows::clean_close::{LaunchCheck, LaunchPath, WalkInputs};

fn canon(p: &Path) -> PathBuf {
    dunce::canonicalize(p).unwrap()
}

#[test]
fn a_pool_owned_protected_item_is_handed_to_the_lawyer_before_its_deny() {
    let w = world("apply-owners", &[]);
    let claude = w
        .dir
        .file("matter/sub/CLAUDE.md", "# restored from the set-aside\n");
    let settings = w.dir.file("matter/a/.claude/settings.json", "{}");
    let notes = w.dir.file("matter/sub/notes.md", "the agent's\n");
    let (claude, settings, notes) = (canon(&claude), canon(&settings), canon(&notes));
    let folder = settings.parent().unwrap().to_owned();
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    for path in [&claude, &folder, &settings, &notes] {
        acl.owners.insert(path.clone(), USER_SID.to_owned());
    }
    let mut plan = launch_plan(&w, &[]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    for path in [&claude, &folder, &settings] {
        assert_eq!(acl.owners[path], LAWYER, "{}", path.display());
    }
    assert_eq!(
        acl.owners[&notes], USER_SID,
        "the agent's own file stays its own"
    );
    for path in [&claude, &folder] {
        assert!(
            has(&acl, path, USER_SID, Rights::DenyWrite),
            "{}",
            path.display()
        );
    }
    assert!(acl.handed_trees.contains(&folder), "{:?}", acl.handed_trees);
}

/// S15 `PR4a2`: a crash after a protected folder's deny is written and before its owner pass
/// leaves the entry pending, so the next launch gives that tree its owner pass even when the
/// check skips the own-tree walks.
#[test]
fn a_pending_denys_tree_gets_its_owner_pass_when_the_check_skips() {
    let w = world("apply-owners-pending", &[]);
    let x = canon(
        &w.dir
            .file("matter/a/.claude/commands/x.md", "the agent's\n"),
    );
    let folder = canon(&w.at("matter/a/.claude"));
    let mut acl = FakeAcl::default();
    acl.owners.insert(x.clone(), USER_SID.to_owned());
    acl.panic_walk = Some(folder.clone());
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        let mut plan = launch_plan(&w, &[]);
        apply(&mut ledger(&w), &mut plan, &mut acl, &mut around(&w))
    }));
    assert!(crashed.is_err(), "the launch died in the owner pass");
    acl.panic_walk = None;
    assert_eq!(acl.owners[&x], USER_SID);
    let mut ledger = ledger(&w);
    let entry = ledger.entries().find(|e| e.path == folder).unwrap();
    assert_eq!(entry.state, EntryState::Pending);

    let mut plan = launch_plan(&w, &[]);
    let check = LaunchCheck::fast(WalkInputs::of_plan(&plan), Vec::new());
    assert_eq!(check.path(), LaunchPath::Skipped);
    apply_checked(&mut ledger, &mut plan, &mut acl, &mut around(&w), &check).unwrap();
    assert_eq!(acl.owners[&x], LAWYER);
    assert!(acl.handed_trees.contains(&folder), "{:?}", acl.handed_trees);
}

/// S15 `PR4a2`: on a full walk every protected tree gets its owner pass, so a pool user's file
/// the lawyer moved deep into a `.git` already denied goes to the lawyer.
#[test]
fn a_full_walk_hands_back_a_pool_owned_file_deep_in_a_denied_git() {
    let w = world("apply-owners-full", &[]);
    let object = canon(&w.dir.file("matter/r/.git/objects/ab/cdef", "blob"));
    let git = canon(&w.at("matter/r/.git"));
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    acl.owners.insert(object.clone(), USER_SID.to_owned());
    acl.handed_trees.clear();

    let applied = apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    );
    assert_eq!(applied.unwrap().written, 0, "the deny was there already");
    assert_eq!(acl.owners[&object], LAWYER);
    assert!(acl.handed_trees.contains(&git), "{:?}", acl.handed_trees);
}
