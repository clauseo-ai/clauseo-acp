use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::*;
use crate::windows::FolderSupport;
use crate::windows::acl_plan::Grant;
use crate::windows::acl_plan::plan;
use crate::windows::admit::{FolderList, ReadDir, admit_with};
use crate::windows::fence::{MatterFolders, Neighbours};
use crate::windows::ledger::{EntryState, ledger_path};
use crate::windows::test_world::{
    FakeFolders, GROUP_SID, NOTES, OTHER_POOL_SIDS, STATE, USER_SID, World, matter_one, matter_two,
    reserved, sids, trusted,
};
use crate::windows::writer::{Label, Strict};

type Key = (PathBuf, String, Rights, Inherit);

fn key(grant: &Grant) -> Key {
    (
        grant.path().to_owned(),
        grant.sid().as_str().to_owned(),
        grant.rights(),
        grant.inherit(),
    )
}

/// The permission writes, in memory: the entries present, the walks asked for, and failures
/// to give.
/// What runs before each fake write.
type OnAdd = Box<dyn FnMut(&Grant)>;

#[derive(Default)]
struct FakeAcl {
    aces: BTreeSet<Key>,
    fail_add: Option<PathBuf>,
    fail_walk: Option<(PathBuf, ApplyFailure)>,
    walks: Vec<(PathBuf, Reach, bool)>,
    /// The lawyer's own explicit labels, by path.
    labels: BTreeMap<PathBuf, Label>,
    /// The labels put back, in order.
    restored: Vec<(PathBuf, Label)>,
    /// The child the last failed walk refused.
    refused: Option<PathBuf>,
    /// The trees handed back, with the pool users named.
    reclaimed: Vec<(PathBuf, Vec<Sid>)>,
    /// `fail_walk` fails once only.
    fail_once: bool,
    /// A walk of this root panics, as a crash would stop it.
    panic_walk: Option<PathBuf>,
    /// A removal panics.
    panic_remove: bool,
    /// Runs before each write, as another process acting meanwhile.
    on_add: Option<OnAdd>,
}

/// What a launch needs from around the matter, faked: the real listing, every folder `Ok`,
/// the set-asides asked for, and the credentials file.
#[derive(Debug, Default)]
struct Around {
    set_aside: Vec<PathBuf>,
    credentials: Option<PathBuf>,
}

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Ok(FolderSupport::Ok)
    }

    fn set_aside(&mut self, path: &Path) -> io::Result<()> {
        self.set_aside.push(path.to_owned());
        Ok(())
    }

    fn credentials(&self) -> Option<PathBuf> {
        self.credentials.clone()
    }
}

/// The surroundings of `w`'s matter: its credential link is the credentials file.
fn around(w: &World) -> Around {
    Around {
        credentials: Some(w.at(&format!("{STATE}/auth.json"))),
        ..Around::default()
    }
}

impl AclWriter for FakeAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        if let Some(hook) = self.on_add.as_mut() {
            hook(grant);
        }
        if self.fail_add.as_deref() == Some(grant.path()) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        Ok(if self.aces.insert(key(grant)) {
            Added::Written
        } else {
            Added::AlreadyThere
        })
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        assert!(!self.panic_remove, "a crash during a removal");
        self.aces.remove(&key(grant));
        Ok(())
    }

    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        Ok(self.aces.contains(&key(grant)))
    }

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), reach, strict.is_some()));
        assert!(
            self.panic_walk.as_deref() != Some(root),
            "a crash during a walk"
        );
        if let Some((at, failure)) = self.fail_walk.clone()
            && at == root
            && strict.is_some()
        {
            if self.fail_once {
                self.fail_walk = None;
            }
            self.refused = Some(root.join("child"));
            return Err(failure);
        }
        Ok(Walked {
            visited: 1,
            ..Walked::default()
        })
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        self.refused.take()
    }

    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), Reach::Tree, false));
        self.reclaimed.push((root.to_owned(), pool.to_vec()));
        Ok(Walked::default())
    }

    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool> {
        Ok(a.file_name() == b.file_name())
    }

    fn label(&mut self, path: &Path) -> io::Result<Option<Label>> {
        Ok(self.labels.get(path).copied())
    }

    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure> {
        self.restored.push((grant.path().to_owned(), label));
        Ok(())
    }
}

/// A world with siblings `sib1`, `sib2` (and any of `more`) beside the matter.
fn world(test: &str, more: &[&str]) -> World {
    let w = World::new(test);
    for name in ["sib1", "sib2"].iter().chain(more) {
        w.dir.dir(name);
    }
    w
}

fn ledger(w: &World) -> Ledger {
    Ledger::open(ledger_path(&w.at("data")), &trusted(), &reserved()).unwrap()
}

/// Matter one's plan, with `siblings` listed beside it.
fn launch_plan(w: &World, siblings: &[&str]) -> AdmittedPlan {
    let policy = w.full_policy();
    let folders = FakeFolders {
        listing: siblings.iter().map(|s| w.canonical(s)).collect(),
        ..FakeFolders::default()
    };
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let admitted =
        admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &folders).unwrap();
    plan(&admitted, &sids(), matter_one()).unwrap()
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn has(acl: &FakeAcl, path: &Path, sid: &str, rights: Rights) -> bool {
    acl.aces
        .iter()
        .any(|(p, s, r, _)| p == path && s == sid && *r == rights)
}

#[test]
fn a_launch_lists_every_entry_applied_with_fix_1_and_fix_2_per_matter() {
    let w = world("apply-launch", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = launch_plan(&w, &["sib1", "sib2"]);
    let applied = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    assert!(applied.written > 0);
    assert_eq!(ledger.entries().count(), plan.grants().len());
    assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
    // Missing protected names get no entry.
    let missing = w.canonical("matter").join(".agents");
    assert!(ledger.entries().all(|e| e.path != missing));
    // Fix 1: every other pool user denied on the matter, state and notes folders.
    for other in OTHER_POOL_SIDS {
        for folder in ["matter", STATE, NOTES] {
            let path = w.canonical(folder);
            assert!(
                has(&acl, &path, other, Rights::DenyReadWrite),
                "{other} {folder}"
            );
        }
    }
    assert!(!has(
        &acl,
        &w.canonical("matter"),
        USER_SID,
        Rights::DenyReadWrite
    ));
    // Fix 2: the group denied reading each sibling.
    for sib in ["sib1", "sib2"] {
        assert!(
            has(&acl, &w.canonical(sib), GROUP_SID, Rights::DenyRead),
            "{sib}"
        );
    }
    // The matter's own tree and each new sibling tree are walked, strictly.
    for strict_root in ["matter", "sib1", "sib2", STATE, NOTES] {
        let root = w.canonical(strict_root);
        let walked = acl.walks.iter().any(|(r, _, s)| *r == root && *s);
        assert!(walked, "{strict_root} walked strictly");
    }
}

#[test]
fn the_next_launch_writes_nothing_and_still_walks_the_matter_tree() {
    let w = world("apply-again", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let before: Vec<EntryId> = ledger.entries().map(|e| e.id).collect();
    acl.walks.clear();

    let applied = apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(applied.written, 0);
    assert_eq!(ledger.entries().map(|e| e.id).collect::<Vec<_>>(), before);
    let roots: Vec<&PathBuf> = acl.walks.iter().map(|(r, _, _)| r).collect();
    assert!(
        roots.contains(&&w.canonical("matter")),
        "re-propagated: {roots:?}"
    );
    assert!(
        !roots.contains(&&w.canonical("sib1")),
        "unchanged sibling not walked"
    );
}

#[test]
fn a_failed_write_rolls_back_only_the_new_entries_of_this_launch() {
    let w = world("apply-rollback", &["sib3", "sib4"]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let (aces, entries) = (acl.aces.clone(), ledger.entries().count());

    acl.fail_add = Some(w.canonical("sib4"));
    let error = apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1", "sib3", "sib4"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            Error::AclApply {
                path_kind: PathKind::Sibling,
                failure: ApplyFailure::Io(io::ErrorKind::PermissionDenied),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        acl.aces, aces,
        "sib3's new entry removed, the earlier ones kept"
    );
    assert_eq!(ledger.entries().count(), entries);
    let sib3 = w.canonical("sib3");
    assert!(
        acl.walks.iter().any(|(r, _, s)| *r == sib3 && !s),
        "sib3 walked again"
    );
}

#[test]
fn a_protected_child_fails_the_walk_rolls_back_and_leaves_no_entry() {
    let w = world("apply-protected", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_walk = Some((w.canonical("matter"), ApplyFailure::ProtectedChild));
    let error = apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        Error::AclApply {
            failure: ApplyFailure::ProtectedChild,
            ..
        }
    ));
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
    assert_eq!(ledger.entries().count(), 0);
}

#[test]
fn an_explicit_everyone_child_in_a_sibling_refuses_the_launch() {
    let w = world("apply-everyone", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_walk = Some((w.canonical("sib2"), ApplyFailure::BroadAllowChild));
    let error = apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1", "sib2"]),
        &mut acl,
        &mut around(&w),
    );
    assert!(matches!(
        error,
        Err(Error::AclApply {
            path_kind: PathKind::Sibling,
            failure: ApplyFailure::BroadAllowChild,
            ..
        })
    ));
    assert!(acl.aces.is_empty());
    assert_eq!(ledger.entries().count(), 0);
}

/// A sibling deny on `w`'s `sib1`, as matter two's launch plans it.
fn sibling_deny(w: &World) -> Grant {
    Grant {
        path: w.canonical("sib1"),
        sid: sid(GROUP_SID),
        rights: Rights::DenyRead,
        inherit: Inherit::Tree,
        kind: PathKind::Sibling,
    }
}

#[test]
fn a_rollback_leaves_an_entry_another_matters_entry_names() {
    let w = world("apply-shared", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    // Matter two listed the same deny, pending: its write never happened.
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![sibling_deny(&w)]);
    ledger.record_pending(&mut two).unwrap();

    acl.fail_add = Some(w.canonical(NOTES));
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap_err();
    assert!(
        acl.aces.contains(&key(&sibling_deny(&w))),
        "shared: left in place"
    );
    let left: Vec<MatterId> = ledger.entries().map(|e| e.matter).collect();
    assert_eq!(
        left,
        vec![matter_two()],
        "matter one's entries dropped, two's kept"
    );
}

#[test]
fn a_sibling_that_becomes_a_matter_loses_its_deny_once_its_first_launch_succeeds() {
    let w = world("apply-lift", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1", "sib2"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    // Matter two shares sib2's deny.
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![sibling_deny(&w)]);
    ledger.record_pending(&mut two).unwrap();

    // sib1 becomes matter two's folder.
    let modify = Grant {
        path: w.canonical("sib1"),
        sid: sid(OTHER_POOL_SIDS[0]),
        rights: Rights::Modify,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    let mut plan = AdmittedPlan::from_grants(matter_two(), vec![modify]);
    let applied = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(applied.lifted, 2, "both matters' entries for the deny");
    assert!(!has(
        &acl,
        &w.canonical("sib1"),
        GROUP_SID,
        Rights::DenyRead
    ));
    assert!(has(&acl, &w.canonical("sib2"), GROUP_SID, Rights::DenyRead));
    let sib1 = w.canonical("sib1");
    assert!(
        ledger
            .entries()
            .all(|e| !(e.path == sib1 && e.kind == PathKind::Sibling))
    );
    assert!(acl.walks.iter().any(|(r, _, s)| *r == sib1 && !s));
}

#[test]
fn forgetting_a_matter_removes_its_entries_but_a_shared_one_and_walks_each_tree_once() {
    let w = world("apply-forget", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![sibling_deny(&w)]);
    ledger.record_pending(&mut two).unwrap();
    acl.walks.clear();

    let done = forget_matter(&mut ledger, matter_one(), &mut acl).unwrap();
    assert_eq!(done.shared, 1);
    assert_eq!(done.kept, 0);
    let left: Vec<Key> = acl.aces.iter().cloned().collect();
    assert_eq!(left, vec![key(&sibling_deny(&w))]);
    let matter = w.canonical("matter");
    let walks = acl.walks.iter().filter(|(r, _, _)| *r == matter).count();
    assert_eq!(walks, 1, "{:?}", acl.walks);
    assert!(acl.walks.iter().all(|(_, _, strict)| !strict));
    assert!(ledger.entries().all(|e| e.matter == matter_two()));
}

#[test]
fn reconcile_keeps_a_pending_entry_whose_entry_is_there() {
    let w = world("apply-reconcile", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let deny = sibling_deny(&w);
    let other = Grant {
        path: w.canonical("sib2"),
        ..deny.clone()
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![deny.clone(), other]);
    ledger.record_pending(&mut plan).unwrap();
    acl.aces.insert(key(&deny));
    let done = reconcile(&mut ledger, &mut acl).unwrap();
    assert_eq!((done.applied, done.dropped), (1, 1));
}

#[test]
fn growing_the_pool_adds_the_new_users_denies_to_every_registered_matter() {
    let w = world("apply-grow", &[]);
    w.dir.dir("other/m2.state");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let registered = [
        RegisteredMatter {
            id: matter_one(),
            folders: MatterFolders {
                root: w.at("matter"),
                state: w.at(STATE),
                notes: Some(w.at(NOTES)),
            },
            pool_user: sid(USER_SID),
            fenced: true,
        },
        RegisteredMatter {
            id: matter_two(),
            folders: MatterFolders {
                root: w.at("other/m2"),
                state: w.at("other/m2.state"),
                notes: None,
            },
            pool_user: sid(OTHER_POOL_SIDS[0]),
            fenced: true,
        },
    ];
    let small = [sid(USER_SID), sid(OTHER_POOL_SIDS[0])];
    let first =
        fence_registered(&mut ledger, &registered, &small, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(first.written, 3 + 2, "each matter fenced from the other");

    let grown = [small[0].clone(), small[1].clone(), sid(OTHER_POOL_SIDS[1])];
    let next =
        fence_registered(&mut ledger, &registered, &grown, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(
        next.written,
        3 + 2,
        "only the new user's denies, on every folder"
    );
    let newcomer = OTHER_POOL_SIDS[1];
    for folder in ["matter", STATE, NOTES, "other/m2", "other/m2.state"] {
        assert!(has(
            &acl,
            &w.canonical(folder),
            newcomer,
            Rights::DenyReadWrite
        ));
    }
    let owners: BTreeSet<MatterId> = ledger.entries().map(|e| e.matter).collect();
    assert_eq!(owners, [matter_one(), matter_two()].into_iter().collect());
}

mod review;
