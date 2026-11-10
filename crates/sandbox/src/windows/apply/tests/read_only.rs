//! A read of another matter never writes in it (R18, H1), at every level of the grant: the whole
//! folder (its fence deny on the folder alone lifted, a deny of writes standing in), a folder in
//! it, a folder holding a refused name, and one file. Each write the pool user could try is
//! checked against the modelled access check (`reads/access.rs`), in the matter's folder and in
//! each folder below, and so is a plant of an agent's instructions (`CLAUDE.md`, `AGENTS.md`,
//! `.claude\`). A crash at any step of the lift or of its restore leaves no write open either.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lifts::{
    assert_lifted, assert_restored, launch, marked, pool, reads, root_deny, root_writable,
    stand_in, volume,
};
use super::reads::access::{APPEND, DELETE, POOL_TOKEN, READ, USERS, WRITE, granted};
use super::*;

/// `FILE_WRITE_EA`.
const WRITE_EA: u32 = 0x10;
/// `FILE_DELETE_CHILD`.
const DELETE_CHILD: u32 = 0x40;
/// `FILE_WRITE_ATTRIBUTES`.
const WRITE_ATTRIBUTES: u32 = 0x100;
/// `WRITE_DAC`.
const WRITE_DAC: u32 = 0x4_0000;
/// `WRITE_OWNER`.
const WRITE_OWNER: u32 = 0x8_0000;

/// Every folder in matter two's tree, refused names' included.
const FOLDERS: [&str; 9] = [
    "other/m2",
    "other/m2/docs",
    "other/m2/docs/deep",
    "other/m2/sub",
    "other/m2/sub/src",
    "other/m2/wt",
    "other/m2/.git",
    "other/m2/sub/.git",
    "other/m2/.Claude",
];

/// Every file in it, with its folder.
const FILES: [(&str, &str); 9] = [
    ("other/m2/a.txt", "other/m2"),
    ("other/m2/docs/b.txt", "other/m2/docs"),
    ("other/m2/docs/deep/c.txt", "other/m2/docs/deep"),
    ("other/m2/sub/e.txt", "other/m2/sub"),
    ("other/m2/sub/src/d.rs", "other/m2/sub/src"),
    ("other/m2/wt/f.txt", "other/m2/wt"),
    ("other/m2/wt/.git", "other/m2/wt"),
    ("other/m2/.git/config", "other/m2/.git"),
    ("other/m2/.Claude/settings.json", "other/m2/.Claude"),
];

/// Whether matter one's pool user is granted `want` on `rel`.
fn can(w: &World, acl: &FakeAcl, rel: &str, want: u32) -> bool {
    granted(&acl.aces, &w.canonical(rel), &POOL_TOKEN, want)
}

/// Asserts matter one's pool user can change nothing in matter two's tree: in no folder can it
/// make a file or a folder (a rename into it is one of those), remove a child, or change the
/// folder; no file can it write, append to, delete, rename away or change.
fn assert_shut(w: &World, acl: &FakeAcl) {
    let changes = [
        ("set its attributes", WRITE_ATTRIBUTES),
        ("set its extended attributes", WRITE_EA),
        ("change its permissions", WRITE_DAC),
        ("take it", WRITE_OWNER),
    ];
    for folder in FOLDERS {
        let adds = [
            ("make a file or rename one in", WRITE),
            ("make a folder or rename one in", APPEND),
            ("remove a child", DELETE_CHILD),
        ];
        for (what, bits) in adds.iter().chain(&changes) {
            assert!(!can(w, acl, folder, *bits), "{what} in {folder}");
        }
        let parent = folder.rsplit_once('/').unwrap().0;
        let deleted = can(w, acl, folder, DELETE) || can(w, acl, parent, DELETE_CHILD);
        assert!(!deleted, "delete or rename {folder}");
    }
    for (file, folder) in FILES {
        let writes = [("write", WRITE), ("append", APPEND)];
        for (what, bits) in writes.iter().chain(&changes) {
            assert!(!can(w, acl, file, *bits), "{what} {file}");
        }
        let deleted = can(w, acl, file, DELETE) || can(w, acl, folder, DELETE_CHILD);
        assert!(!deleted, "delete or rename {file}");
    }
}

/// Asserts no agent's instructions can be planted anywhere in matter two's tree: a `CLAUDE.md`
/// or an `AGENTS.md` is a new file in a folder, a `.claude\` a new folder.
fn assert_no_plant(w: &World, acl: &FakeAcl) {
    for folder in FOLDERS {
        for name in ["CLAUDE.md", "AGENTS.md"] {
            assert!(!can(w, acl, folder, WRITE), "{folder}\\{name}");
        }
        assert!(!can(w, acl, folder, APPEND), "{folder}\\.claude");
    }
}

/// Matter two's own pool user still writes there: the checks above aren't vacuous.
fn assert_owner_writes(w: &World, acl: &FakeAcl) {
    let own = [OTHER_POOL_SIDS[0], GROUP_SID, USERS];
    for folder in ["other/m2", "other/m2/docs", "other/m2/sub"] {
        let at = w.canonical(folder);
        assert!(granted(&acl.aces, &at, &own, WRITE | APPEND), "{folder}");
    }
}

#[test]
fn no_level_of_a_read_writes_or_plants_anything_in_the_other_matter() {
    let levels: [&[(&str, bool)]; 5] = [
        &[("other/m2", false)],
        &[("other/m2/docs", false)],
        &[("other/m2/sub", false)],
        &[("other/m2/a.txt", true)],
        &[("other/m2/docs", false), ("other/m2/sub/e.txt", true)],
    ];
    for (n, level) in levels.into_iter().enumerate() {
        let w = World::new(&format!("read-only-{n}"));
        w.other_tree();
        let (mut ledger, mut acl) = (ledger(&w), volume(&w));
        launch(&w, &mut ledger, &mut acl, level);
        // What the read allows it reads: the grant took effect.
        let (first, _) = level[0];
        assert!(reads(&w, &acl, first), "{first}");
        assert_eq!(marked(&w, &ledger), first == "other/m2", "{level:?}");
        assert_shut(&w, &acl);
        assert_no_plant(&w, &acl);
        assert_owner_writes(&w, &acl);
        // And after the run.
        let ended = end_reads(
            &mut ledger,
            matter_one(),
            &pool(),
            &mut acl,
            &mut around(&w),
        );
        ended.unwrap();
        assert_shut(&w, &acl);
        assert_no_plant(&w, &acl);
    }
}

#[test]
fn the_whole_folders_read_lists_it_with_its_deny_of_writes_standing_in() {
    let w = World::new("read-only-lifted");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    assert_lifted(&w, &ledger, &acl);
    assert!(
        can(&w, &acl, "other/m2", READ),
        "lists and reads its attributes"
    );
    // Neither a file nor a folder may be made in it, and it may not be deleted or renamed.
    for want in [WRITE, APPEND, DELETE] {
        assert!(!can(&w, &acl, "other/m2", want), "{want:#x}");
    }
    // The stand-in is the folder's alone: what is below keeps the fence's own deny.
    let (path, sid, rights, inherit) = stand_in(&w);
    assert_eq!((rights, inherit), (Rights::DenyWrite, Inherit::ThisOnly));
    assert_eq!(rights.mask(), 0x000d_0156);
    let below = (path, sid, Rights::DenyReadWrite, Inherit::Below);
    assert!(acl.aces.contains(&below));
    // The run's end swaps the original back.
    end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_restored(&w, &ledger, &acl);
}

/// Whether `grant` is the entry on matter two's folder alone (`root`) for matter one's pool user
/// with `rights`.
fn on_root(root: &Path, grant: &Grant, rights: Rights) -> bool {
    grant.path() == root
        && grant.sid().as_str() == USER_SID
        && grant.inherit() == Inherit::ThisOnly
        && grant.rights() == rights
}

/// Runs `step`, which a crash stops (a panic out of the fake); reopens the ledger.
fn crashed(w: &World, step: impl FnOnce()) -> Ledger {
    assert!(catch_unwind(AssertUnwindSafe(step)).is_err());
    super::ledger(w)
}

#[test]
fn a_crash_at_the_stand_ins_write_leaves_the_deny_and_the_folder_shut() {
    let w = World::new("read-only-crash-stand-in");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    let root = w.canonical("other/m2");
    acl.on_add = Some(Box::new(move |g| {
        assert!(
            !on_root(&root, g, Rights::DenyWrite),
            "a crash at the write"
        );
    }));
    let mut ledger = crashed(&w, || {
        launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    });
    acl.on_add = None;
    assert!(marked(&w, &ledger));
    assert!(acl.aces.contains(&root_deny(&w)) && !acl.aces.contains(&stand_in(&w)));
    assert!(!reads(&w, &acl, "other/m2") && !root_writable(&w, &acl));
    assert_shut(&w, &acl);
    reconcile(&mut ledger, &mut acl).unwrap();
    assert_restored(&w, &ledger, &acl);
}

#[test]
fn a_crash_at_the_restores_write_leaves_the_stand_in_and_the_folder_unwritable() {
    let w = World::new("read-only-crash-back");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    let root = w.canonical("other/m2");
    acl.on_add = Some(Box::new(move |g| {
        assert!(
            !on_root(&root, g, Rights::DenyReadWrite),
            "a crash at the write"
        );
    }));
    let mut ledger = crashed(&w, || {
        drop(end_reads(
            &mut ledger,
            matter_one(),
            &pool(),
            &mut acl,
            &mut around(&w),
        ));
    });
    acl.on_add = None;
    // The lifted state, no worse: the folder's names list, nothing writes.
    assert_lifted(&w, &ledger, &acl);
    assert_shut(&w, &acl);
    reconcile(&mut ledger, &mut acl).unwrap();
    assert_restored(&w, &ledger, &acl);
}

#[test]
fn a_crash_at_the_stand_ins_removal_leaves_both_denies_and_the_folder_shut() {
    let w = World::new("read-only-crash-removal");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    acl.panic_remove = true;
    let mut ledger = crashed(&w, || {
        drop(end_reads(
            &mut ledger,
            matter_one(),
            &pool(),
            &mut acl,
            &mut around(&w),
        ));
    });
    acl.panic_remove = false;
    assert!(marked(&w, &ledger));
    assert!(acl.aces.contains(&root_deny(&w)) && acl.aces.contains(&stand_in(&w)));
    assert!(!reads(&w, &acl, "other/m2") && !root_writable(&w, &acl));
    assert_shut(&w, &acl);
    reconcile(&mut ledger, &mut acl).unwrap();
    assert_restored(&w, &ledger, &acl);
}

#[test]
fn forgetting_the_read_matter_takes_the_stand_in_with_its_fence() {
    let w = World::new("read-only-forget-fenced");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    forget_matter(&mut ledger, matter_two(), &mut acl, &mut around(&w)).unwrap();
    assert!(
        !acl.aces.contains(&stand_in(&w)),
        "no stand-in outlives the fence"
    );
    assert!(!marked(&w, &ledger));
}
