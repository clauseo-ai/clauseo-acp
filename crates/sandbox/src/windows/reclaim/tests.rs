//! `--reclaim`'s targets, from the ledger alone: an owed hand-back gives its matter's own trees,
//! each refusal kind its item, a matter with no pool user nothing; none walked twice; the roots
//! it refuses; and the lawyer's hand-back after it.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use crate::test_support::TempDir;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Rights};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{RecordedFile, ledger_path};
use crate::windows::matters::MatterRow;
use crate::windows::test_world::{USER_SID, matter_one, matter_two, reserved, trusted};
use crate::windows::write::write_atomic;
use crate::windows::writer::{Added, FileId, Label, Reach, Strict, Walked};

/// `name` as an absolute path on every OS.
pub(super) fn abs(name: &str) -> PathBuf {
    std::env::temp_dir()
        .join("clauseo-reclaim-paths")
        .join(name)
}

fn grant(path: &Path, kind: PathKind, rights: Rights, inherit: Inherit) -> Grant {
    Grant {
        path: path.to_owned(),
        sid: Sid::parse(USER_SID).unwrap(),
        rights,
        inherit,
        kind,
    }
}

/// The credential link, in matter one's state folder.
fn link() -> PathBuf {
    abs("s1/.credentials.json")
}

/// Matter one's own folders (`m1`, its state and notes), a read-only item in it, a sibling deny,
/// the credential link and the Claude store, recorded; then `extra` merged into the file.
pub(super) fn ledger_with(temp: &TempDir, extra: &Value) -> Ledger {
    let path = ledger_path(temp.path());
    let grants = vec![
        grant(&abs("m1"), PathKind::Matter, Rights::Modify, Inherit::Tree),
        grant(&abs("s1"), PathKind::State, Rights::Modify, Inherit::Tree),
        grant(&abs("n1"), PathKind::Notes, Rights::Modify, Inherit::Tree),
        grant(
            &abs("m1/.git"),
            PathKind::ReadOnlyInMatter,
            Rights::DenyWrite,
            Inherit::Tree,
        ),
        grant(
            &abs("sib"),
            PathKind::Sibling,
            Rights::DenyReadWrite,
            Inherit::Tree,
        ),
        // Our own allows that only this item gets: the credential link in the state folder,
        // and the Claude store outside every matter.
        grant(
            &link(),
            PathKind::CredentialLink,
            Rights::Modify,
            Inherit::ThisOnly,
        ),
        grant(
            &abs("store"),
            PathKind::ClaudeStore,
            Rights::Modify,
            Inherit::ThisOnly,
        ),
    ];
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants);
    let mut ledger = Ledger::open(&path, &trusted(), &reserved()).unwrap();
    ledger.record_pending(&mut plan).unwrap();
    let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (key, value) in extra.as_object().unwrap() {
        file[key] = value.clone();
    }
    write_atomic(&path, &serde_json::to_vec(&file).unwrap()).unwrap();
    Ledger::open(&path, &trusted(), &reserved()).unwrap()
}

/// `matters.json`'s rows: matter one holds a pool user, matter two released its.
pub(super) fn rows() -> Rows {
    let held = MatterRow {
        pool_user: Some(Sid::parse(USER_SID).unwrap()),
        ..MatterRow::default()
    };
    let released = MatterRow {
        released_at: Some(1),
        ..held.clone()
    };
    Rows::from([(matter_one(), held), (matter_two(), released)])
}

fn stuck(matter: MatterId, path: &Path, why: &str) -> Value {
    json!({ "matter": matter.to_string(), "path": path, "why": why })
}

fn roots(found: &[ReclaimRoot]) -> Vec<(PathBuf, bool, Why)> {
    found
        .iter()
        .map(|r| (r.root.clone(), r.whole, r.why))
        .collect()
}

#[test]
fn nothing_owed_and_nothing_refused_walks_nothing() {
    let temp = TempDir::new("reclaim-nothing");
    let ledger = ledger_with(&temp, &json!({}));
    assert_eq!(targets(&ledger, &rows()), []);
}

#[test]
fn an_owed_hand_back_walks_its_matters_own_trees_whole() {
    let temp = TempDir::new("reclaim-owed");
    let owed = json!([{ "matter": matter_one().to_string(), "pool": USER_SID }]);
    let ledger = ledger_with(&temp, &json!({ "handbacks": owed }));
    let found = targets(&ledger, &rows());
    let want = ["m1", "n1", "s1"].map(|n| (abs(n), true, Why::Owed));
    let mut got = roots(&found);
    got.sort();
    assert_eq!(got, want);
    assert!(found.iter().all(|r| r.matter == matter_one()));
}

#[test]
fn each_refusal_kind_gives_its_item_and_a_root_alone() {
    let temp = TempDir::new("reclaim-kinds");
    let (child, shut, root) = (abs("m1/moved/doc.txt"), abs("s1/planted"), abs("n1"));
    let reclaims = json!([
        stuck(matter_one(), &child, "pool_owned_child"),
        stuck(matter_one(), &shut, "child_shut"),
        stuck(matter_one(), &root, "pool_owned_root"),
    ]);
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims }));
    let found = targets(&ledger, &rows());
    let mut got = roots(&found);
    got.sort();
    let want = vec![
        (child, true, Why::Stuck(Stuck::PoolOwnedChild)),
        (root, false, Why::Stuck(Stuck::PoolOwnedRoot)),
        (shut, true, Why::Stuck(Stuck::ChildShut)),
    ];
    assert_eq!(got, want);
}

#[test]
fn a_matter_without_a_pool_user_or_an_owed_hand_back_is_left_out() {
    let temp = TempDir::new("reclaim-released");
    let reclaims = json!([stuck(matter_two(), &abs("m1/doc.txt"), "pool_owned_child")]);
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims }));
    assert_eq!(targets(&ledger, &rows()), []);

    // Its forget still owes a hand-back: then its refusal counts too.
    let temp = TempDir::new("reclaim-released-owed");
    let owed = json!([{ "matter": matter_two().to_string(), "pool": USER_SID }]);
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims, "handbacks": owed }));
    assert_eq!(targets(&ledger, &rows()).len(), 1);
}

#[test]
fn an_item_inside_a_tree_walked_whole_isnt_walked_again() {
    let temp = TempDir::new("reclaim-nested");
    let owed = json!([{ "matter": matter_one().to_string(), "pool": USER_SID }]);
    let reclaims = json!([
        stuck(matter_one(), &abs("m1/moved"), "pool_owned_child"),
        stuck(matter_one(), &abs("m1"), "pool_owned_root"),
        stuck(matter_one(), &abs("m1"), "child_shut"),
    ]);
    let ledger = ledger_with(&temp, &json!({ "handbacks": owed, "reclaims": reclaims }));
    let found = targets(&ledger, &rows());
    let m1: Vec<_> = found
        .iter()
        .filter(|r| r.root.starts_with(abs("m1")))
        .collect();
    assert_eq!(m1.len(), 1, "{found:?}");
    assert!(m1[0].whole);
}

/// The owner writes the ledger, so a recorded item outside every matter root it records (a
/// system folder, another user's folder, a matter it doesn't record, the Claude store) is
/// never a target.
#[test]
fn a_refusal_outside_every_recorded_matter_is_never_a_target() {
    let temp = TempDir::new("reclaim-outside");
    let outside = [
        "Windows/System32",
        "Users/colleague/Documents",
        "m2/doc.txt",
        "store",
        "sib",
    ];
    let reclaims: Vec<Value> = outside
        .iter()
        .map(|o| stuck(matter_one(), &abs(o), "child_shut"))
        .collect();
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims }));
    assert_eq!(targets(&ledger, &rows()), []);
    let mut bounds = matter_roots(&ledger);
    bounds.sort();
    assert_eq!(bounds, ["m1", "n1", "s1"].map(abs));
}

/// Our own allows are kept by the hand-back after: each target lists those at or below it, and
/// a refused launch's target keeps the matter's own pool user's.
#[test]
fn a_target_keeps_our_own_allows_and_the_matters_own_pool_user() {
    let temp = TempDir::new("reclaim-keep");
    let owed = json!([{ "matter": matter_one().to_string(), "pool": USER_SID }]);
    let ledger = ledger_with(&temp, &json!({ "handbacks": owed }));
    let found = targets(&ledger, &rows());
    let state = found.iter().find(|r| r.root == abs("s1")).unwrap();
    let user = Sid::parse(USER_SID).unwrap();
    assert!(
        state.keep.contains(&(link(), user.clone())),
        "{:?}",
        state.keep
    );
    assert!(state.keep.contains(&(abs("s1"), user.clone())));
    assert!(
        state.own.is_empty(),
        "a matter being forgotten keeps no pool user's"
    );
    assert!(
        found
            .iter()
            .all(|r| !r.keep.iter().any(|(p, _)| *p == abs("store")))
    );

    let temp = TempDir::new("reclaim-keep-stuck");
    let reclaims = json!([stuck(matter_one(), &abs("s1/sub"), "child_shut")]);
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims }));
    let found = targets(&ledger, &rows());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].own, [user]);
    assert_eq!(found[0].keep, [], "nothing of ours below s1/sub");
}

#[test]
fn a_kept_or_relative_refusal_is_never_a_target() {
    let temp = TempDir::new("reclaim-kept");
    let reclaims = json!([
        { "matter": matter_one().to_string(), "path": "relative/doc.txt", "why": "child_shut" },
        { "matter": matter_one().to_string(), "path": abs("x"), "why": "a newer kind" },
        "not an object",
    ]);
    let ledger = ledger_with(&temp, &json!({ "reclaims": reclaims }));
    assert_eq!(ledger.reclaims_owed(), []);
    assert_eq!(targets(&ledger, &rows()), []);
}

#[test]
fn a_root_outside_the_bounds_a_volume_root_or_a_pool_profile_is_refused() {
    let bounds = [abs("m1"), abs("Users/ClauseoSb01/m")];
    let profiles = [abs("Users/ClauseoSb01")];
    assert!(admissible(&abs("m1/doc.txt"), &bounds, &profiles));
    assert!(admissible(&abs("m1"), &bounds, &profiles), "a bound itself");
    assert!(
        !admissible(&abs("m2/doc.txt"), &bounds, &profiles),
        "outside every bound"
    );
    assert!(
        !admissible(&abs("m10"), &bounds, &profiles),
        "a name that only starts like one"
    );
    assert!(
        !admissible(&abs("Users/ClauseoSb01/m/x"), &bounds, &profiles),
        "in a profile"
    );
    assert!(!admissible(Path::new("relative"), &bounds, &profiles));
    let volume = abs("m1").ancestors().last().unwrap().to_owned();
    assert!(!admissible(
        &volume,
        std::slice::from_ref(&volume),
        &profiles
    ));
    assert!(
        !admissible(&abs("m1"), &[abs("")], &[abs("m1/Users/ClauseoSb02")]),
        "holds one"
    );
}

/// On Windows: the verbatim form and case don't change the answer.
#[cfg(windows)]
#[test]
fn admissible_compares_verbatim_and_case_variants_alike() {
    let bounds = [PathBuf::from(r"C:\m\Matter One")];
    let profiles = [PathBuf::from(r"C:\Users\ClauseoSb01")];
    for inside in [
        r"\\?\C:\m\Matter One\doc.txt",
        r"c:\M\matter one\Doc.txt",
        r"\\?\c:\M\MATTER ONE",
    ] {
        assert!(
            admissible(Path::new(inside), &bounds, &profiles),
            "{inside}"
        );
    }
    let verbatim_bounds = [PathBuf::from(r"\\?\C:\M\MATTER ONE")];
    assert!(admissible(
        Path::new(r"C:\m\Matter One\x"),
        &verbatim_bounds,
        &profiles
    ));
    for outside in [
        r"\\?\C:\m\Matter Two",
        r"C:\USERS\clauseosb01\x",
        r"\\?\C:\",
    ] {
        assert!(
            !admissible(Path::new(outside), &bounds, &profiles),
            "{outside}"
        );
    }
}

/// One strip: its root, the pool SIDs it strips, what it keeps, whether whole.
type Strip = (PathBuf, Vec<Sid>, Vec<(PathBuf, Sid)>, bool);

/// What one strip was given to take denies off: the items taken, and the SIDs kept.
pub(super) type Denies = (Vec<(PathBuf, FileId)>, Vec<Sid>);

/// Records the hand-backs; refuses at `shut`. Any other call is counted in `other`, and fails.
#[derive(Default)]
pub(super) struct Hands {
    pub(super) roots: Vec<PathBuf>,
    pub(super) trees: Vec<PathBuf>,
    /// Each strip: its root, the pool SIDs it strips, what it keeps, whether whole.
    pub(super) strips: Vec<Strip>,
    /// What each strip was given to take denies off: the items taken, and the SIDs kept.
    pub(super) denies: Vec<Denies>,
    /// The pool SIDs each root's hand-back was given.
    pub(super) handed: Vec<Vec<Sid>>,
    pub(super) shut: Option<PathBuf>,
    pub(super) refused: Option<PathBuf>,
    pub(super) other: usize,
}

impl Hands {
    /// A call the hand-back must never make.
    fn other<T>(&mut self) -> Result<T, ApplyFailure> {
        self.other += 1;
        Err(ApplyFailure::Io(io::ErrorKind::Unsupported))
    }

    /// [`Hands::other`], as an `io::Result`.
    fn other_io<T>(&mut self) -> io::Result<T> {
        self.other += 1;
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl AclWriter for Hands {
    fn add(&mut self, _: &Grant) -> Result<Added, ApplyFailure> {
        self.other()
    }
    fn remove(&mut self, _: &Grant) -> Result<(), ApplyFailure> {
        self.other()
    }
    fn present(&mut self, _: &Grant) -> io::Result<bool> {
        self.other_io()
    }
    fn propagate(
        &mut self,
        _: &Path,
        _: Reach,
        _: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.other()
    }
    fn same_file(&mut self, _: &Path, _: &Path) -> io::Result<bool> {
        self.other_io()
    }
    fn file_id(&mut self, _: &Path) -> io::Result<Option<FileId>> {
        self.other_io()
    }
    fn remove_recorded(
        &mut self,
        _: &RecordedFile,
        _: &Grant,
        _: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        self.other()
    }
    fn present_recorded(&mut self, _: &RecordedFile, _: &Grant) -> io::Result<bool> {
        self.other_io()
    }
    fn add_expecting(&mut self, _: &Grant, _: &Path) -> Result<Added, ApplyFailure> {
        self.other()
    }
    fn refused_at(&mut self) -> Option<PathBuf> {
        self.refused.take()
    }
    fn reclaim(&mut self, _: &Path, _: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.other()
    }
    fn hand_back(&mut self, path: &Path, pool: &[Sid]) -> Result<bool, ApplyFailure> {
        self.roots.push(path.to_owned());
        self.handed.push(pool.to_vec());
        Ok(false)
    }
    fn hand_back_tree(&mut self, root: &Path, _: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.trees.push(root.to_owned());
        if self.shut.as_deref() == Some(root) {
            self.refused = Some(root.join("inner"));
            return Err(ApplyFailure::ChildShut);
        }
        Ok(Walked::default())
    }
    fn label(&mut self, _: &Path) -> io::Result<Option<Label>> {
        self.other_io()
    }
    fn restore_label(&mut self, _: &Grant, _: Label) -> Result<(), ApplyFailure> {
        self.other()
    }
    fn strip_taken(
        &mut self,
        root: &Path,
        others: &[Sid],
        keep: &[(PathBuf, Sid)],
        whole: bool,
        denies: &DenyStrip<'_>,
    ) -> Result<Walked, ApplyFailure> {
        let entry = (root.to_owned(), others.to_vec(), keep.to_vec(), whole);
        self.strips.push(entry);
        self.denies
            .push((denies.taken.items(), denies.kept.to_vec()));
        Ok(Walked::default())
    }
}

const OTHER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1002";

fn target(name: &str, whole: bool, own: &[&str], keep: &[(PathBuf, Sid)]) -> ReclaimRoot {
    ReclaimRoot {
        matter: matter_one(),
        root: abs(name),
        whole,
        why: Why::Owed,
        own: own.iter().map(|s| Sid::parse(s).unwrap()).collect(),
        keep: keep.to_vec(),
    }
}

#[test]
fn the_lawyers_hand_back_goes_over_each_target_and_lists_a_refusal() {
    let targets = [
        target("a", true, &[], &[]),
        target("b", false, &[], &[]),
        target("c", true, &[], &[]),
    ];
    let mut acl = Hands {
        shut: Some(abs("a")),
        ..Hands::default()
    };
    let pool = [Sid::parse(USER_SID).unwrap()];

    let done = hand_back_targets(&mut acl, &targets, &pool, &DenyStrip::none());

    assert_eq!(acl.roots, [abs("a"), abs("b"), abs("c")]);
    assert_eq!(
        acl.trees,
        [abs("a"), abs("c")],
        "a root alone has no tree walk"
    );
    let stripped: Vec<(PathBuf, bool)> = acl.strips.iter().map(|s| (s.0.clone(), s.3)).collect();
    // The strip goes first, so the refused tree walk at `a` came after its strip.
    assert_eq!(
        stripped,
        [(abs("a"), true), (abs("b"), false), (abs("c"), true)],
        "each strip before its tree walk"
    );
    assert_eq!(acl.other, 0, "nothing but the hand-backs");
    assert_eq!(done.done, 2);
    assert_eq!(
        done.refused,
        [(ApplyFailure::ChildShut, Some(abs("a").join("inner")))]
    );
}

/// A taken root (`PoolOwnedRoot`) of a matter with its own pool user: every hand-back is given
/// the pool less that user, so its `Modify` stays and another's allow goes; our own allows go
/// down as `keep`.
#[test]
fn the_matters_own_pool_user_and_our_own_allows_are_kept() {
    let (own, other) = (Sid::parse(USER_SID).unwrap(), Sid::parse(OTHER).unwrap());
    let keep = [(link(), own.clone())];
    let targets = [target("n1", false, &[USER_SID], &keep)];
    let mut acl = Hands::default();

    let done = hand_back_targets(
        &mut acl,
        &targets,
        &[own, other.clone()],
        &DenyStrip::none(),
    );

    assert_eq!(done.done, 1);
    assert_eq!(acl.handed, [vec![other.clone()]]);
    assert_eq!(acl.strips, [(abs("n1"), vec![other], keep.to_vec(), false)]);
}
