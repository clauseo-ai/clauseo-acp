//! A launch after the launch check (S15 `PR3d`): the own-tree walks it leaves out, the ones it
//! keeps, and the record it removes whatever happens next.

use super::*;
use crate::windows::clean_close::fake::{FakeJournal, journal_for};
use crate::windows::clean_close::{
    Closed, FullReason, LaunchCheck, LaunchPath, WalkInputs, check_launch, session_cursor,
    write_clean_close,
};
use crate::windows::set_aside::Checked;

/// `USN_REASON_SECURITY_CHANGE`.
const SECURITY: u32 = 0x800;

/// `USN_REASON_FILE_CREATE`.
const CREATE: u32 = 0x100;

/// A matter launched once (siblings `sib1`, `sib2`), its session ended clean, and a fake
/// journal over its own trees: the matter (ID 100), then the state and notes folders.
struct Launched {
    w: World,
    ledger: Ledger,
    acl: FakeAcl,
    journal: FakeJournal,
    roots: Vec<PathBuf>,
}

impl Launched {
    fn new(test: &str) -> Launched {
        Launched::with(test, &[])
    }

    /// [`Launched::new`] with the lawyer's `files`, below the matter, there at the first launch.
    fn with(test: &str, files: &[&str]) -> Launched {
        let w = world(test, &[]);
        for rel in files {
            w.dir.file(&format!("matter/{rel}"), "# the lawyer's\n");
        }
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let mut plan = launch_plan(&w, &["sib1", "sib2"]);
        apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
        // The matter, the state folder, the notes folder.
        let inputs = WalkInputs::of_plan(&plan);
        let roots: Vec<PathBuf> = [PathKind::Matter, PathKind::State, PathKind::Notes]
            .iter()
            .map(|kind| {
                inputs
                    .roots
                    .iter()
                    .find(|r| r.1 == *kind)
                    .unwrap()
                    .0
                    .clone()
            })
            .collect();
        let ids: Vec<(PathBuf, u64)> = roots.iter().cloned().zip([100, 200, 300]).collect();
        let journal = journal_for(&ids, 100);
        let cursor = session_cursor(&ledger, matter_one(), &journal).unwrap();
        let closed = write_clean_close(
            &w.at("data"),
            &ledger,
            matter_one(),
            &journal,
            Some(&cursor),
            &Ok(Walked::default()),
            &Ok(Checked::default()),
        );
        assert_eq!(closed, Closed::Written);
        acl.walks.clear();
        Launched {
            w,
            ledger,
            acl,
            journal,
            roots,
        }
    }

    fn plan(&self) -> AdmittedPlan {
        launch_plan(&self.w, &["sib1", "sib2"])
    }

    fn check(&self) -> LaunchCheck {
        check_launch(
            &self.w.at("data"),
            &self.ledger,
            &self.plan(),
            &self.journal,
        )
        .unwrap()
    }

    fn apply(&mut self, check: &LaunchCheck) -> Result<Applied, Error> {
        let mut plan = self.plan();
        let mut around = around(&self.w);
        apply_checked(
            &mut self.ledger,
            &mut plan,
            &mut self.acl,
            &mut around,
            check,
        )
    }

    /// The walks of an own tree, or of anything in one.
    fn own_walks(&self) -> Vec<(PathBuf, Reach, bool)> {
        self.acl
            .walks
            .iter()
            .filter(|(path, _, _)| self.roots.iter().any(|root| in_folder(path, root)))
            .cloned()
            .collect()
    }
}

#[test]
fn a_launch_with_nothing_changed_walks_no_own_tree() {
    let mut l = Launched::new("checked-skip");
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Skipped);
    let applied = l.apply(&check).unwrap();
    assert_eq!(applied.written, 0);
    assert_eq!(l.own_walks(), Vec::new());
    assert!(l.ledger.entries().all(|e| e.state == EntryState::Applied));
    // The full walk, for comparison, walks all three.
    let full = LaunchCheck::full(FullReason::NoRecord);
    l.apply(&full).unwrap();
    assert_eq!(l.own_walks().len(), 3);
}

#[test]
fn a_targeted_launch_walks_only_the_changed_folders_strictly() {
    let mut l = Launched::new("checked-targeted");
    l.journal.change(100_005, 100, SECURITY, false);
    let state = l.roots[1].clone();
    l.journal.change(201, 200, SECURITY, true);
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Targeted(3));
    l.apply(&check).unwrap();
    let mut walks = l.own_walks();
    walks.sort_by_key(|w| (w.0.clone(), format!("{:?}", w.1)));
    let mut want = vec![
        (l.roots[0].clone(), Reach::Children, true),
        (state.clone(), Reach::Children, true),
        (state.join("a"), Reach::Tree, true),
    ];
    want.sort_by_key(|w| (w.0.clone(), format!("{:?}", w.1)));
    assert_eq!(walks, want);
}

#[test]
fn a_grant_not_yet_written_is_still_written_and_walked_when_the_check_skips() {
    let mut l = Launched::new("checked-new-grant");
    let sib = l.w.canonical("sib1");
    l.acl.aces.retain(|(path, ..)| *path != sib);
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Skipped);
    let applied = l.apply(&check).unwrap();
    assert!(applied.written > 0);
    assert!(
        l.acl
            .walks
            .iter()
            .any(|(path, _, strict)| *path == sib && *strict)
    );
    assert_eq!(l.own_walks(), Vec::new());
}

#[test]
fn a_check_made_for_other_walk_inputs_walks_in_full() {
    let mut l = Launched::new("checked-other-plan");
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Skipped);
    // Applied to a plan with another pool user denied on the matter: its inputs differ.
    let mut grants = l.plan().grants().to_vec();
    grants.push(Grant {
        path: l.roots[0].clone(),
        sid: sid("S-1-5-21-1111111111-2222222222-3333333333-1099"),
        rights: Rights::DenyReadWrite,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    });
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants);
    let mut around = around(&l.w);
    apply_checked(&mut l.ledger, &mut plan, &mut l.acl, &mut around, &check).unwrap();
    assert_eq!(l.own_walks().len(), 3);
}

#[test]
fn the_record_is_gone_even_when_the_launch_then_fails() {
    let mut l = Launched::new("checked-fails");
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Skipped);
    let sib = l.w.canonical("sib2");
    l.acl.aces.retain(|(path, ..)| *path != sib);
    l.acl.fail_add = Some(sib);
    assert!(l.apply(&check).is_err());
    assert_eq!(l.check().path(), LaunchPath::Full(FullReason::NoRecord));
}

#[test]
fn forgetting_the_matter_removes_its_clean_close_record() {
    let mut l = Launched::new("checked-forget");
    let path = crate::windows::clean_close::fake_record_path(&l.w.at("data"), &matter_one());
    assert!(path.is_file());
    let mut around = around(&l.w);
    forget_matter(&mut l.ledger, matter_one(), &mut l.acl, &mut around).unwrap();
    assert!(!path.exists());
}

/// The state of the ledger's entry for the pool user's write-deny on `rel` in the matter, and
/// whether the ACL holds that deny.
fn nested_deny(l: &Launched, rel: &str) -> (Option<EntryState>, bool) {
    let path = l.roots[0].join(rel);
    let entry = l.ledger.entries().find(|e| {
        e.path == path
            && e.kind == PathKind::ReadOnlyInMatter
            && e.rights == Rights::DenyWrite
            && e.sid == sid(USER_SID)
    });
    let held = has(&l.acl, &path, USER_SID, Rights::DenyWrite);
    (entry.map(|e| e.state), held)
}

/// S15 `PR4a2`: each nested protected name there at the first launch gets the pool user's
/// write-deny, recorded `applied`. A later launch the check skips writes nothing and walks no
/// own tree, and every deny still stands: nothing changed, so the plan lists the same items.
#[test]
fn nested_denies_stand_when_the_check_skips() {
    let mut l = Launched::with(
        "checked-nested-skip",
        &[
            "sub/CLAUDE.md",
            "sub/.claude/settings.json",
            "a/b/agents.md",
            "a/.mcp.json",
            "deep/x/.git/config",
        ],
    );
    let denied = [
        "sub/CLAUDE.md",
        "sub/.claude",
        "a/b/agents.md",
        "a/.mcp.json",
        "deep/x/.git",
    ];
    for rel in denied {
        assert_eq!(
            nested_deny(&l, rel),
            (Some(EntryState::Applied), true),
            "{rel}"
        );
    }
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Skipped);
    let applied = l.apply(&check).unwrap();
    assert_eq!(applied.written, 0);
    assert_eq!(l.own_walks(), Vec::new());
    for rel in denied {
        assert_eq!(
            nested_deny(&l, rel),
            (Some(EntryState::Applied), true),
            "{rel}"
        );
    }
}

/// S15 `PR4a2`: a nested `CLAUDE.md` the lawyer made between sessions makes the launch
/// targeted at its folder; the policy's walk at that launch finds it, and its deny is written,
/// walked strictly and recorded.
#[test]
fn a_nested_claude_md_made_between_sessions_gets_its_deny_on_a_targeted_launch() {
    let mut l = Launched::new("checked-nested-targeted");
    assert_eq!(nested_deny(&l, "a/CLAUDE.md"), (None, false));
    l.w.dir.file("matter/a/CLAUDE.md", "# the lawyer's, new\n");
    // A new file in folder `a` (ID 101).
    l.journal.change(100_900, 101, CREATE, false);
    let check = l.check();
    assert_eq!(check.path(), LaunchPath::Targeted(1));
    let applied = l.apply(&check).unwrap();
    assert!(applied.written > 0);
    assert_eq!(
        nested_deny(&l, "a/CLAUDE.md"),
        (Some(EntryState::Applied), true)
    );
    let claude = l.roots[0].join("a/CLAUDE.md");
    let walks = l.own_walks();
    assert!(walks.contains(&(l.roots[0].join("a"), Reach::Children, true)));
    assert!(walks.contains(&(claude, Reach::Tree, true)), "{walks:?}");
}

/// S15 `PR4a2`: forgetting the matter hands its nested denies back like every other entry.
#[test]
fn forgetting_the_matter_hands_back_its_nested_denies() {
    let mut l = Launched::with(
        "checked-nested-forget",
        &["sub/CLAUDE.md", "a/.Claude/settings.json"],
    );
    for rel in ["sub/CLAUDE.md", "a/.Claude"] {
        assert_eq!(
            nested_deny(&l, rel),
            (Some(EntryState::Applied), true),
            "{rel}"
        );
    }
    let mut around = around(&l.w);
    forget_matter(&mut l.ledger, matter_one(), &mut l.acl, &mut around).unwrap();
    for rel in ["sub/CLAUDE.md", "a/.Claude"] {
        assert_eq!(nested_deny(&l, rel), (None, false), "{rel}");
    }
    assert_eq!(l.ledger.entries().count(), 0);
}
