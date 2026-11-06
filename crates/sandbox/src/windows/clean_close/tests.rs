//! The launch check and the clean-close record, through a fake journal, on every OS.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::fake::{FakeJournal, found, journal_for};
use super::*;
use crate::windows::FolderSupport;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights, Sid, plan};
use crate::windows::admit::{PathKind, admit_with};
use crate::windows::fence::Neighbours;
use crate::windows::ledger::{ByPath, Ledger, Take, ledger_path};
use crate::windows::set_aside::{self, Checked};
use crate::windows::test_world::{FakeFolders, World, matter_one, reserved, sids, trusted};
use crate::windows::writer::{Reach, Walked};

mod cases;
mod record_file;

/// `USN_REASON_SECURITY_CHANGE`.
pub(super) const SECURITY: u32 = 0x800;
/// `USN_REASON_FILE_CREATE`.
pub(super) const CREATE: u32 = 0x100;
/// `USN_REASON_RENAME_NEW_NAME`.
pub(super) const RENAME_NEW: u32 = 0x2000;
/// `USN_REASON_DATA_EXTEND | USN_REASON_CLOSE`.
pub(super) const WRITE_CLOSE: u32 = 0x2 | 0x8000_0000;

/// A matter launched once, its entries applied, and a fake journal over its own trees: the
/// matter (ID 100), and the state and notes folders (200, 300) in the order the plan lists
/// them, each with `files` files.
pub(super) struct Scene {
    pub(super) w: World,
    pub(super) data: PathBuf,
    pub(super) ledger: Ledger,
    pub(super) plan: AdmittedPlan,
    pub(super) journal: FakeJournal,
    pub(super) matter: MatterId,
    /// The own roots, with their IDs.
    pub(super) roots: Vec<(PathBuf, u64)>,
}

/// Matter one's plan.
pub(crate) fn launch_plan(w: &World) -> AdmittedPlan {
    let policy = w.full_policy();
    let folders = FakeFolders::default();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let admitted =
        admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &folders).unwrap();
    plan(&admitted, &sids(), matter_one()).unwrap()
}

impl Scene {
    pub(super) fn new(test: &str) -> Scene {
        Scene::with_files(test, 100)
    }

    pub(super) fn with_files(test: &str, files: u64) -> Scene {
        let w = World::new(test);
        let data = w.at("data");
        let mut ledger = Ledger::open(ledger_path(&data), &trusted(), &reserved()).unwrap();
        let mut recorded = launch_plan(&w);
        ledger.record_pending(&mut recorded).unwrap();
        ledger.mark_walked(recorded.entries()).unwrap();
        let plan = launch_plan(&w);
        let roots: Vec<(PathBuf, u64)> = WalkInputs::of_plan(&plan)
            .roots
            .iter()
            .map(|(path, kind)| {
                let kinds = [PathKind::Matter, PathKind::State, PathKind::Notes];
                let at = kinds.iter().position(|k| k == kind).unwrap();
                let id = [100, 200, 300][at];
                (path.clone(), id)
            })
            .collect();
        assert_eq!(roots.len(), 3, "matter, state and notes");
        let journal = journal_for(&roots, files);
        Scene {
            w,
            data,
            ledger,
            plan,
            journal,
            matter: matter_one(),
            roots,
        }
    }

    /// The matter folder's path.
    pub(super) fn matter_root(&self) -> PathBuf {
        self.root_of(100)
    }

    pub(super) fn root_of(&self, id: u64) -> PathBuf {
        self.roots.iter().find(|r| r.1 == id).unwrap().0.clone()
    }

    pub(super) fn cursor(&self) -> Cursor {
        session_cursor(&self.ledger, self.matter, &self.journal).expect("a cursor")
    }

    /// A clean session end: the cursor, the repair and the after-session pass passed.
    pub(super) fn close(&self) -> Closed {
        let cursor = self.cursor();
        self.close_with(
            Some(&cursor),
            &Ok(Walked::default()),
            &Ok(Checked::default()),
        )
    }

    pub(super) fn close_with(
        &self,
        cursor: Option<&Cursor>,
        repair: &Result<Walked, crate::Error>,
        after: &Result<Checked, crate::Error>,
    ) -> Closed {
        let (data, ledger, journal) = (&self.data, &self.ledger, &self.journal);
        write_clean_close(data, ledger, self.matter, journal, cursor, repair, after)
    }

    pub(super) fn launch(&self) -> LaunchCheck {
        check_launch(&self.data, &self.ledger, &self.plan, &self.journal).unwrap()
    }

    pub(super) fn launch_path(&self) -> LaunchPath {
        self.launch().path()
    }

    pub(super) fn record_exists(&self) -> bool {
        record::record_path(&self.data, &self.matter).exists()
    }

    /// Opens a session of the matter in the set-aside's record.
    pub(super) fn open_session(&self) {
        set_aside::open_session(&self.data, &self.matter, "s1");
    }

    /// Leaves the matter's `Modify` entry on its folder `removing`: its removal failed.
    pub(super) fn leave_removing(&mut self) {
        let root = self.matter_root();
        let pick = |e: &crate::windows::ledger::Entry| {
            (e.path == root && e.rights == Rights::Modify).then_some(Take::Remove)
        };
        let failed = |_: &crate::windows::ledger::Entry| Err(io::Error::other("refused"));
        self.ledger.take_back(pick, failed).unwrap();
    }

    /// Records a hand-back owed for the matter, as a forget does before its removals: through
    /// an allow entry away from the own roots, dropped, so nothing near them is `removing`.
    pub(super) fn owe_handback(&mut self) {
        let failed = |_: &crate::windows::ledger::Entry| Err(io::Error::other("refused"));
        let matter = self.matter;
        let roots: Vec<PathBuf> = self.roots.iter().map(|r| r.0.clone()).collect();
        let away = |path: &Path| {
            !roots
                .iter()
                .any(|r| crate::windows::acl_plan::in_folder(path, r) || r.starts_with(path))
        };
        let pick = |e: &crate::windows::ledger::Entry| {
            (e.matter == matter && e.rights.is_allow() && away(&e.path)).then_some(Take::Drop)
        };
        self.ledger
            .take_back_with(Some(matter), pick, &mut ByPath, failed)
            .unwrap();
    }

    /// Records a new grant on the matter's folder, `pending`: its walk is owed.
    pub(super) fn leave_pending(&mut self) {
        let grant = Grant {
            path: self.matter_root(),
            sid: Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1099").unwrap(),
            rights: Rights::DenyReadWrite,
            inherit: Inherit::Tree,
            kind: crate::windows::admit::PathKind::Matter,
        };
        let mut more = AdmittedPlan::from_grants(self.matter, vec![grant]);
        self.ledger.record_pending(&mut more).unwrap();
    }
}

#[test]
fn the_launch_and_session_end_compute_the_same_walk_inputs() {
    let s = Scene::new("clean-inputs");
    assert_eq!(
        WalkInputs::of_plan(&s.plan),
        WalkInputs::of_ledger(&s.ledger, s.matter)
    );
}

#[test]
fn nothing_changed_since_a_clean_close_skips() {
    let mut s = Scene::new("clean-skip");
    assert_eq!(s.close(), Closed::Written);
    // A content write and an unrelated change elsewhere on the volume don't count.
    s.journal.change(100_000, 100, WRITE_CLOSE, false);
    s.journal.change(9_999_999, 8_888_888, SECURITY, false);
    let check = s.launch();
    assert_eq!(check.path(), LaunchPath::Skipped);
    assert_eq!(check.targets(), &[]);
    assert_eq!(s.journal.handed.get(), 2, "the read went past the cursor");
}

#[test]
fn an_edit_made_between_close_and_launch_never_skips() {
    let mut s = Scene::new("clean-edit-between");
    assert_eq!(s.close(), Closed::Written);
    // The lawyer gives a file in the matter's folder a broad allow while the app is closed.
    s.journal.change(100_005, 100, SECURITY, false);
    let check = s.launch();
    assert_eq!(check.path(), LaunchPath::Targeted(1));
    let target = &check.targets()[0];
    assert_eq!(target.root, s.matter_root());
    assert_eq!(target.parts, Vec::<std::ffi::OsString>::new());
    assert_eq!(target.reach, Reach::Children);
}

#[test]
fn a_change_through_another_name_walks_each_folder_the_record_holds_it_in() {
    let mut s = Scene::new("clean-hard-link");
    assert_eq!(s.close(), Closed::Written);
    // A DACL edit through a name outside the trees: the record's parent is the outside folder.
    s.journal.change(102, 77_777, SECURITY, false);
    let check = s.launch();
    assert_eq!(check.path(), LaunchPath::Targeted(1));
    assert_eq!(
        check.targets()[0].parts,
        vec![std::ffi::OsString::from("a")]
    );
}

#[test]
fn a_folder_moved_in_gets_its_whole_subtree_walked_and_its_folder_its_children() {
    let mut s = Scene::new("clean-move-in");
    assert_eq!(s.close(), Closed::Written);
    let moved = s.matter_root().join("a").join("brought");
    s.journal.places.insert(555, found(moved));
    s.journal.change(555, 101, RENAME_NEW, true);
    // Its contents come with no records; a file made in it later counts through it.
    s.journal.change(556, 555, CREATE, false);
    let check = s.launch();
    let reaches: Vec<(Vec<std::ffi::OsString>, Reach)> = check
        .targets()
        .iter()
        .map(|t| (t.parts.clone(), t.reach))
        .collect();
    assert_eq!(
        reaches,
        vec![
            (vec!["a".into()], Reach::Children),
            (vec!["a".into(), "brought".into()], Reach::Tree),
        ]
    );
}

#[test]
fn a_change_to_an_own_root_itself_walks_in_full() {
    let mut s = Scene::new("clean-root-touched");
    assert_eq!(s.close(), Closed::Written);
    s.journal.change(100, 50, SECURITY, true);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::RootTouched));
}

#[test]
fn a_folder_gone_or_moved_out_or_a_link_now_is_left_to_its_folders_walk() {
    let mut s = Scene::new("clean-gone");
    assert_eq!(s.close(), Closed::Written);
    // Folder `a` of the state folder moved out of the trees.
    let state = s.root_of(200);
    s.journal.places.insert(201, found(s.w.at("elsewhere/a")));
    s.journal.change(201, 200, 0x1000, true);
    let check = s.launch();
    assert_eq!(check.path(), LaunchPath::Targeted(1));
    assert_eq!(check.targets()[0].root, state);
    assert_eq!(check.targets()[0].parts, Vec::<std::ffi::OsString>::new());
}

#[test]
fn the_set_aside_is_told_the_matter_folders_only_and_nothing_for_a_full_walk() {
    let mut s = Scene::new("clean-touched");
    assert_eq!(s.close(), Closed::Written);
    s.journal.change(102, 101, SECURITY, false);
    s.journal.change(200_005, 200, SECURITY, false);
    let check = s.launch();
    assert_eq!(check.path(), LaunchPath::Targeted(2));
    let touched = check.touched_in_matter().unwrap();
    assert_eq!(touched, vec![(vec![std::ffi::OsString::from("a")], false)]);
    assert_eq!(
        LaunchCheck::full(FullReason::NoRecord).touched_in_matter(),
        None
    );
    assert_eq!(s.launch().touched_in_matter(), None);
}

/// S15 `PR4a`: a nested `CLAUDE.md` or `AGENTS.md` made, renamed in or removed while the app was
/// closed is a name change in a folder the record holds, so the launch never skips: the
/// set-aside re-probes that folder for every name in its nested list. The journal's records
/// carry no names, so no name can be passed over here.
#[test]
fn a_nested_instruction_file_made_renamed_in_or_removed_while_closed_never_skips() {
    // `USN_REASON_FILE_DELETE`.
    const DELETE: u32 = 0x200;
    // `a/CLAUDE.md` made (900), `a/AGENTS.md` renamed in from outside the trees (901), and
    // the file in `a` (102) removed.
    for (test, file, parent, reason) in [
        ("clean-nested-create", 900, 101, CREATE),
        ("clean-nested-rename", 901, 101, RENAME_NEW),
        ("clean-nested-delete", 102, 101, DELETE),
    ] {
        let mut s = Scene::new(test);
        assert_eq!(s.close(), Closed::Written);
        s.journal.change(file, parent, reason, false);
        let check = s.launch();
        assert_eq!(check.path(), LaunchPath::Targeted(1), "{test}");
        let touched = check.touched_in_matter().unwrap();
        assert_eq!(touched, vec![(vec![std::ffi::OsString::from("a")], false)]);
    }
}
