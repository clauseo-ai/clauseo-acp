//! The reads of other matters' folders (R18) through a launch, the run's end, a narrowing and a
//! crash, with the access check modelled (`access.rs`) under matter two's fence.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::fake::Broad;
use super::*;
use crate::windows::ledger::Entry;

pub(in crate::windows::apply::tests) mod access;

use access::{POOL_TOKEN, READ, USERS, WRITE, granted, volume_default};

/// Matter one's session's SIDs: its pool user.
fn pool() -> Vec<Sid> {
    vec![sid(USER_SID)]
}

/// A fake with the volume's default entry (`Users` may read and write everything) at the top,
/// whose strict walks refuse an allow beating a fence, as `NtfsAcl`'s do.
fn volume(w: &World) -> FakeAcl {
    let mut acl = FakeAcl::default();
    let top = dunce::canonicalize(w.dir.path()).unwrap();
    acl.aces.insert(volume_default(&top));
    acl.broad = Broad::Refused;
    acl
}

/// Matter one's launch reading each of `reads` in matter two's folder, whose fence it applies.
fn launch(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl, reads: &[(&str, bool)]) -> Applied {
    launch_with(w, ledger, acl, reads, false)
}

/// [`launch`], with matter two passed as `fenced` already or not.
fn launch_with(
    w: &World,
    ledger: &mut Ledger,
    acl: &mut FakeAcl,
    reads: &[(&str, bool)],
    fenced: bool,
) -> Applied {
    let policy = w.reading(reads);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let mut others = w.neighbour_m2();
    others.registered[0].fenced = fenced;
    let data = w.at("data");
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    apply(ledger, &mut plan, acl, &mut around(w)).unwrap()
}

/// Whether matter one's pool user may read `rel` (relative to the temp folder).
fn reads(w: &World, acl: &FakeAcl, rel: &str) -> bool {
    granted(&acl.aces, &w.canonical(rel), &POOL_TOKEN, READ)
}

/// The paths of matter one's read entries in the ledger, relative to matter two's folder, in
/// order, each once (a folder has two).
fn read_entries(w: &World, ledger: &Ledger) -> Vec<String> {
    let root = w.canonical("other/m2");
    let reads = ledger
        .entries()
        .filter(|e: &&Entry| e.kind == PathKind::OtherMatterRead);
    reads
        .map(|e| {
            e.path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// Every entry naming matter one's pool user in matter two's folder, but the fence's deny on
/// its root.
fn pool_entries_in_m2(w: &World, acl: &FakeAcl) -> Vec<Key> {
    let root = w.canonical("other/m2");
    let fence = |k: &Key| k.0 == root && k.2 == Rights::DenyReadWrite;
    let aces = acl.aces.iter();
    aces.filter(|k| k.0.starts_with(&root) && k.1 == USER_SID && !fence(k))
        .cloned()
        .collect()
}

#[test]
fn a_read_of_another_matter_reads_its_tree_and_no_refused_name_at_any_depth() {
    let w = World::new("reads-launch");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);

    // The folder itself too: a read of the whole folder lifts its fence deny for this pool user,
    // and a deny of writes stands in for it, so the folder lists and still takes no write.
    let readable = [
        "other/m2",
        "other/m2/a.txt",
        "other/m2/docs",
        "other/m2/docs/b.txt",
        "other/m2/docs/deep/c.txt",
        "other/m2/sub",
        "other/m2/sub/src/d.rs",
        "other/m2/sub/e.txt",
        "other/m2/wt",
        "other/m2/wt/f.txt",
    ];
    for rel in readable {
        assert!(reads(&w, &acl, rel), "{rel}");
        let path = w.canonical(rel);
        assert!(!granted(&acl.aces, &path, &POOL_TOKEN, WRITE), "{rel}");
    }
    let refused = [
        "other/m2/.git",
        "other/m2/.git/config",
        "other/m2/.Claude/settings.json",
        "other/m2/sub/.git",
        "other/m2/sub/.git/HEAD",
        "other/m2/wt/.git",
    ];
    for rel in refused {
        assert!(!reads(&w, &acl, rel), "{rel}");
    }
    // The fence's deny for what is below the folder stays, so a child the read doesn't allow
    // stays closed, and an allow on a child comes before the deny the child inherits.
    // Another pool user reads none of it; matter two's own still reads and writes.
    let other = [OTHER_POOL_SIDS[1], GROUP_SID, USERS];
    let b = w.canonical("other/m2/docs/b.txt");
    assert!(!granted(&acl.aces, &b, &other, READ));
    let own = [OTHER_POOL_SIDS[0], GROUP_SID, USERS];
    assert!(granted(&acl.aces, &b, &own, READ | WRITE));
    // Each tree allowed is walked (here inside the fence's own walk of matter two, new in this
    // launch); no walk starts in a refused name.
    let docs = w.canonical("other/m2/docs");
    assert!(acl.walks.iter().any(|(r, _, _)| docs.starts_with(r)));
    let git = w.canonical("other/m2/.git");
    assert!(acl.walks.iter().all(|(r, _, _)| !r.starts_with(&git)));
}

#[test]
fn a_file_made_during_the_run_in_an_allowed_folder_reads_and_a_folder_made_then_doesnt() {
    for grant in ["other/m2", "other/m2/docs"] {
        let w = World::new("reads-made");
        w.other_tree();
        let (mut ledger, mut acl) = (ledger(&w), volume(&w));
        launch(&w, &mut ledger, &mut acl, &[(grant, false)]);
        w.dir.file("other/m2/docs/new.txt", "n");
        w.dir.file("other/m2/docs/deep/new.txt", "n");
        w.dir.file("other/m2/new.txt", "n");
        assert!(reads(&w, &acl, "other/m2/docs/new.txt"), "{grant}");
        assert!(reads(&w, &acl, "other/m2/docs/deep/new.txt"), "{grant}");
        // Directly in the root a file inherits the fence's deny first.
        assert!(!reads(&w, &acl, "other/m2/new.txt"), "{grant}");
        // A folder made during the run (a `git init`, an agent's folder, a plain one) inherits
        // nothing from the read, nor does what is in it, until the next launch lists it.
        for made in ["docs/.git/HEAD", "docs/deep/.claude/x.md", "docs/new/x.txt"] {
            let file = format!("other/m2/{made}");
            w.dir.file(&file, "n");
            let folder = file.rsplit_once('/').unwrap().0.to_owned();
            assert!(!reads(&w, &acl, &folder), "{grant}: {folder}");
            assert!(!reads(&w, &acl, &file), "{grant}: {file}");
        }
    }
}

#[test]
fn a_git_file_made_during_the_run_reads_but_its_gitdir_target_stays_closed() {
    for grant in ["other/m2", "other/m2/docs"] {
        let w = World::new("reads-made-gitdir");
        w.other_tree();
        w.dir
            .file("other/m2/.git/worktrees/docs/HEAD", "ref: refs/heads/docs");
        let (mut ledger, mut acl) = (ledger(&w), volume(&w));
        launch(&w, &mut ledger, &mut acl, &[(grant, false)]);
        // Made after the grant: a worktree's `.git` file and a `.clauseo` file in an allowed
        // folder take its allow for files; each holds a pointer, not the history.
        w.dir
            .file("other/m2/docs/.git", "gitdir: ../.git/worktrees/docs");
        w.dir.file("other/m2/docs/.clauseo", "x");
        assert!(reads(&w, &acl, "other/m2/docs/.git"), "{grant}");
        assert!(reads(&w, &acl, "other/m2/docs/.clauseo"), "{grant}");
        // What it points at, in the matter's own `.git`, has no allow and keeps the fence.
        let mut closed = vec![
            "other/m2/.git/worktrees/docs",
            "other/m2/.git/worktrees/docs/HEAD",
        ];
        if grant == "other/m2/docs" {
            closed.extend(["other/m2/sub", "other/m2/sub/e.txt"]);
        }
        for rel in closed {
            assert!(!reads(&w, &acl, rel), "{grant}: {rel}");
            let path = w.canonical(rel);
            let allowed = acl
                .aces
                .iter()
                .any(|(p, sid, rights, _)| p == &path && sid == USER_SID && !rights.is_deny());
            assert!(!allowed, "{grant}: {rel}");
        }
        let below = (w.canonical("other/m2"), USER_SID.to_owned());
        let fenced = acl.aces.iter().any(|(p, sid, rights, inherit)| {
            (p, sid) == (&below.0, &below.1)
                && *rights == Rights::DenyReadWrite
                && *inherit == Inherit::Below
        });
        assert!(fenced, "{grant}");
    }
}

#[test]
fn a_read_of_a_folder_in_another_matter_beats_the_fence_inside_it() {
    let w = World::new("reads-child");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    assert!(reads(&w, &acl, "other/m2/docs"));
    assert!(reads(&w, &acl, "other/m2/docs/deep/c.txt"));
    assert!(!reads(&w, &acl, "other/m2/a.txt"));
    assert!(!reads(&w, &acl, "other/m2"));
}

#[test]
fn the_runs_end_takes_every_read_back_and_a_second_end_takes_nothing() {
    let w = World::new("reads-end");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    assert_ne!(read_entries(&w, &ledger), Vec::<String>::new());
    acl.walks.clear();

    // Another session's pool user's end takes none of this session's reads.
    let others = [sid(OTHER_POOL_SIDS[1])];
    let theirs = end_reads(
        &mut ledger,
        matter_one(),
        &others,
        &mut acl,
        &mut around(&w),
    );
    assert_eq!(theirs.unwrap(), Narrowed::default());
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));

    let ended = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert!(ended.removed.contains(&w.canonical("other/m2/docs")));
    assert_eq!(read_entries(&w, &ledger), Vec::<String>::new());
    assert_eq!(pool_entries_in_m2(&w, &acl), Vec::<Key>::new());
    assert!(!reads(&w, &acl, "other/m2/docs/b.txt"));
    assert!(!reads(&w, &acl, "other/m2/a.txt"));
    // The tree allowed is walked again, so its inherited copies go too.
    let docs = w.canonical("other/m2/docs");
    assert!(acl.walks.iter().any(|(r, _, _)| *r == docs));
    // The rest of the launch's entries stay.
    assert!(ledger.entries().any(|e| e.kind == PathKind::Matter));

    let again = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(again, Narrowed::default());

    // The next launch writes them again; matter two's fence is in place, so each tree allowed
    // gets a plain walk of its own, and none starts in a refused name.
    acl.walks.clear();
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    assert!(acl.walks.iter().any(|(r, _, s)| *r == docs && !*s));
    let git = w.canonical("other/m2/sub/.git");
    let m2 = w.canonical("other/m2");
    assert!(
        acl.walks
            .iter()
            .all(|(r, _, _)| !r.starts_with(&git) && *r != m2)
    );
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));
}

#[test]
fn a_forgotten_read_goes_at_once_and_a_second_narrowing_takes_nothing() {
    let w = World::new("reads-narrow");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    let both = [("other/m2/docs", false), ("other/m2/a.txt", true)];
    launch(&w, &mut ledger, &mut acl, &both);
    assert!(reads(&w, &acl, "other/m2/a.txt"));

    let policy = w.reading(&[("other/m2/docs", false)]);
    let narrowed = narrow_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &policy,
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(narrowed.removed, vec![w.canonical("other/m2/a.txt")]);
    assert!(!reads(&w, &acl, "other/m2/a.txt"));
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));
    assert_eq!(read_entries(&w, &ledger), ["docs", "docs/deep"]);

    let entries = ledger.entries().count();
    let again = narrow_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &policy,
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(again, Narrowed::default());
    assert_eq!(ledger.entries().count(), entries);
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));
}

#[test]
fn a_read_narrowed_to_a_folder_inside_it_keeps_that_folders_allows_only() {
    let w = World::new("reads-narrow-in");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);

    let policy = w.reading(&[("other/m2/docs", false)]);
    let narrowed = narrow_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &policy,
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert!(narrowed.removed.contains(&w.canonical("other/m2")));
    assert!(narrowed.removed.contains(&w.canonical("other/m2/sub")));
    assert!(!narrowed.removed.contains(&w.canonical("other/m2/docs")));
    assert_eq!(read_entries(&w, &ledger), ["docs", "docs/deep"]);
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));
    assert!(!reads(&w, &acl, "other/m2/sub/e.txt"));
    assert!(!reads(&w, &acl, "other/m2/a.txt"));
}

#[test]
fn reads_a_crashed_run_left_go_at_the_next_launch_or_the_next_end() {
    let w = World::new("reads-crash");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    // The run crashes: no end.
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    // The next launch, without the read: it is taken back before anything is recorded.
    let mut ledger = super::ledger(&w);
    launch(&w, &mut ledger, &mut acl, &[]);
    assert_eq!(read_entries(&w, &ledger), Vec::<String>::new());
    assert_eq!(pool_entries_in_m2(&w, &acl), Vec::<Key>::new());
    assert!(!reads(&w, &acl, "other/m2/a.txt"));

    // A launch with the read again, then a crash during the run's end, after the entries were
    // marked `removing`: the next end finishes the removal.
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    acl.panic_remove = true;
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        drop(end_reads(
            &mut ledger,
            matter_one(),
            &pool(),
            &mut acl,
            &mut around(&w),
        ));
    }));
    assert!(crashed.is_err());
    acl.panic_remove = false;
    let mut ledger = super::ledger(&w);
    assert!(
        reads(&w, &acl, "other/m2/docs/b.txt"),
        "the allow outlived the crash"
    );
    let ended = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let docs = ["other/m2/docs", "other/m2/docs/deep"].map(|rel| w.canonical(rel));
    assert_eq!(ended.removed, docs);
    assert_eq!(read_entries(&w, &ledger), Vec::<String>::new());
    assert_eq!(pool_entries_in_m2(&w, &acl), Vec::<Key>::new());
}

#[test]
fn a_relaunch_with_the_same_reads_keeps_their_entries() {
    let w = World::new("reads-relaunch");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    let before = read_entries(&w, &ledger);
    let again = launch_with(&w, &mut ledger, &mut acl, &[("other/m2", false)], true);
    assert_eq!(again.written, 0);
    assert!(reads(&w, &acl, "other/m2"));
    assert_eq!(read_entries(&w, &ledger), before);
}

#[test]
fn forgetting_the_matter_takes_its_reads_back() {
    let w = World::new("reads-forget");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(read_entries(&w, &ledger), Vec::<String>::new());
    assert!(!reads(&w, &acl, "other/m2/docs/b.txt"));
}

#[test]
fn a_strict_walk_of_the_other_matter_passes_its_listed_reads_and_refuses_any_other_allow() {
    // Matter two's fence is new in this launch, so its tree is walked strictly with the reads
    // already written: they are listed, so excused.
    let w = World::new("reads-strict");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    let m2 = w.canonical("other/m2");
    assert!(acl.walks.iter().any(|(r, _, strict)| *r == m2 && *strict));

    // An allow for the same pool user the ledger doesn't list (one matter two's agent wrote)
    // refuses the strict walk, as it always did.
    let w = World::new("reads-strict-planted");
    w.other_tree();
    let (mut ledger, mut acl) = (super::ledger(&w), volume(&w));
    let planted = w.canonical("other/m2/docs/b.txt");
    let key = (
        planted,
        USER_SID.to_owned(),
        Rights::ReadAndExecute,
        Inherit::ThisOnly,
    );
    acl.aces.insert(key);
    let policy = w.reading(&[("other/m2/docs", false)]);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let (data, others) = (w.at("data"), w.neighbour_m2());
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    assert!(
        matches!(
            error,
            Error::AclApply {
                failure: ApplyFailure::BroadAllowChild,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_strict_walk_refuses_an_inheriting_allow_where_the_ledger_lists_a_read_of_the_folder_alone() {
    let w = World::new("reads-strict-shape");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    // The read of the whole folder plans `sub` (it holds a refused name) for the folder alone;
    // the same allow made to reach the tree, there before, would open `sub\.git`.
    let sub = w.canonical("other/m2/sub");
    let sid = USER_SID.to_owned();
    acl.aces
        .insert((sub.clone(), sid, Rights::ReadAndExecute, Inherit::Tree));
    let policy = w.reading(&[("other/m2", false)]);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let (data, others) = (w.at("data"), w.neighbour_m2());
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let alone = plan.grants().iter().any(|g| {
        g.kind() == PathKind::OtherMatterRead && g.path() == sub && g.inherit() == Inherit::ThisOnly
    });
    assert!(alone);
    // Matter two's new fence walks its tree strictly with that read listed on `sub` for that
    // SID, the folder alone: the inheriting allow isn't it, and refuses.
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    let failure = ApplyFailure::BroadAllowChild;
    assert!(
        matches!(error, Error::AclApply { failure: f, .. } if f == failure),
        "{error:?}"
    );
}
