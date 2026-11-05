//! [`apply`] on a real NTFS volume, with the ledger: rollbacks leave no entry, forgetting takes
//! everything back, and win-01's timing row. Temp folders and synthetic SIDs.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::windows::NtfsAcl;
use crate::windows::acl_plan::{Grant, Inherit, LOW_LABEL_SID, verbatim};
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::ledger::ledger_path;
use crate::windows::test_acl::{Place, grant, has};
use crate::windows::test_dacl::{drop_privileges, protect_with};
use crate::windows::test_world::{OTHER_POOL_SIDS, USER_SID, matter_one, reserved, trusted};

fn ledger(place: &Place) -> Ledger {
    Ledger::open(ledger_path(&place.at("data")), &trusted(), &reserved()).unwrap()
}

/// The pool user's `Modify`, another pool user's deny, and the label.
fn launch_grants(matter: &Path) -> Vec<Grant> {
    vec![
        grant(matter, USER_SID, Rights::Modify, Inherit::Tree),
        grant(
            matter,
            OTHER_POOL_SIDS[0],
            Rights::DenyReadWrite,
            Inherit::Tree,
        ),
        grant(matter, LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree),
    ]
}

/// Around a temp matter: the real listing, every folder `Ok`, set-asides refused, no
/// credentials file.
struct Around;

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<crate::windows::FolderSupport> {
        Ok(crate::windows::FolderSupport::Ok)
    }

    fn set_aside(&mut self, _matter: clauseo_protocol::MatterId, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

fn launch(place: &Place, matter: &Path) -> Result<Applied, Error> {
    let mut plan = AdmittedPlan::from_grants(matter_one(), launch_grants(matter));
    apply(
        &mut ledger(place),
        &mut plan,
        &mut NtfsAcl::new().unwrap(),
        &mut Around,
    )
}

#[test]
fn a_protected_subfolder_fails_the_launch_and_leaves_no_entry() {
    let place = Place::new("apply-ntfs-protected");
    place.dir.file("matter/sub/doc.txt", "x");
    protect_with(&place.at("matter/sub"), "D:P(A;OICI;FA;;;OW)").unwrap();
    let error = launch(&place, &place.at("matter")).unwrap_err();
    assert!(matches!(
        error,
        Error::AclApply {
            failure: ApplyFailure::ProtectedChild,
            ..
        }
    ));
    for sid in [USER_SID, OTHER_POOL_SIDS[0], LOW_LABEL_SID] {
        assert!(!has(&place.at("matter"), sid, false), "{sid}");
    }
    assert_eq!(ledger(&place).entries().count(), 0);
}

#[test]
fn a_launch_then_forget_leaves_nothing() {
    let place = Place::new("apply-ntfs-forget");
    let doc = place.dir.file("matter/sub/doc.txt", "x");
    let matter = place.at("matter");
    launch(&place, &matter).unwrap();
    assert!(has(&doc, USER_SID, true) && has(&doc, LOW_LABEL_SID, true));
    let mut ledger = ledger(&place);
    let done = forget_matter(
        &mut ledger,
        matter_one(),
        &mut NtfsAcl::new().unwrap(),
        &mut Around,
    )
    .unwrap();
    assert_eq!((done.removed, done.kept), (3, 0));
    for path in [&matter, &doc] {
        for sid in [USER_SID, OTHER_POOL_SIDS[0], LOW_LABEL_SID] {
            assert!(!has(path, sid, true) && !has(path, sid, false), "{sid}");
        }
    }
    assert_eq!(ledger.entries().count(), 0);
}

/// win-01's timing row (ignored on CI): the first propagation over `CLAUSEO_WALK_FILES` files
/// (50,000 by default) in 100 folders, the next launch's re-propagation, and one sibling's
/// deny (fix 2) over a tree as big. Run with `--run-ignored only -E 'test(timing)'`.
#[test]
#[ignore = "win-01 timing row; makes 50,000 files"]
#[expect(
    clippy::disallowed_methods,
    reason = "times the walk; the core never reads a clock"
)]
fn timing_first_propagation_and_fix_costs() {
    let count: usize = std::env::var("CLAUSEO_WALK_FILES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50_000);
    let place = Place::new("apply-ntfs-timing");
    for i in 0..count {
        place
            .dir
            .file(&format!("matter/f{}/doc{i}.txt", i % 100), "x");
        place
            .dir
            .file(&format!("sibling/f{}/doc{i}.txt", i % 100), "x");
    }
    let matter = place.at("matter");
    let started = std::time::Instant::now();
    launch(&place, &matter).unwrap();
    let first = started.elapsed();
    let started = std::time::Instant::now();
    launch(&place, &matter).unwrap();
    let again = started.elapsed();
    let deny = Grant {
        kind: PathKind::Sibling,
        ..grant(
            &place.at("sibling"),
            OTHER_POOL_SIDS[1],
            Rights::DenyRead,
            Inherit::Tree,
        )
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![deny]);
    let started = std::time::Instant::now();
    apply(
        &mut ledger(&place),
        &mut plan,
        &mut NtfsAcl::new().unwrap(),
        &mut Around,
    )
    .unwrap();
    let sibling = started.elapsed();
    println!("files {count}: first {first:?}, next launch {again:?}, one sibling deny {sibling:?}");
}

/// Around a temp matter that sets aside for real: each item is renamed into `aside`, and its
/// path as asked is kept.
struct Moving {
    data: PathBuf,
    asked: Vec<PathBuf>,
}

impl FolderList for Moving {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Moving {
    fn check(&self, _folder: &Path) -> io::Result<crate::windows::FolderSupport> {
        Ok(crate::windows::FolderSupport::Ok)
    }

    fn set_aside(&mut self, matter: clauseo_protocol::MatterId, path: &Path) -> io::Result<()> {
        self.asked.push(path.to_owned());
        // The real set-aside, which renames with `DELETE` only: `std::fs::rename` asks for more,
        // which a file that shuts the lawyer out refuses (round 7, VM run 3).
        let reason = crate::windows::SetAsideReason::LawyerCantOpen;
        crate::windows::set_aside_path(
            &self.data,
            &matter,
            path,
            reason,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )
        .map(drop)
        .map_err(io::Error::other)
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

/// Round 4, rows 4b and 16 of the second VM run: the NTFS walk names a child the lawyer can't
/// open by its verbatim final path, and the launch's roots are plain, so the launch refused
/// `ChildShut` rather than set it aside, and so did forget. Both now set it aside, asked by the
/// path the walk reports, which the test compares in one form (round 9).
#[test]
fn a_child_the_lawyer_cant_open_is_set_aside_by_the_launch_and_by_forget() {
    drop_privileges();
    let place = Place::new("apply-ntfs-shut-aside");
    let matter = place.at("matter");
    let shut = place.dir.file("matter/shut.txt", "x");
    place.dir.dir("data");
    protect_with(&shut, "D:P(A;;FA;;;SY)(A;;0x0;;;OW)").unwrap();
    let mut around = Moving {
        data: place.at("data"),
        asked: Vec::new(),
    };
    let mut ledger = ledger(&place);
    let mut acl = NtfsAcl::new().unwrap();
    let mut plan = AdmittedPlan::from_grants(matter_one(), launch_grants(&matter));
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();
    // The walk reports the verbatim final path (`\\?\C:\…`): both sides compare in that form.
    let asked = |around: &Moving, from: usize| -> Vec<PathBuf> {
        around.asked[from..].iter().map(|p| verbatim(p)).collect()
    };
    assert_eq!(asked(&around, 0), [verbatim(&matter.join("shut.txt"))]);
    assert!(!shut.exists());

    let again = place.dir.file("matter/sub/x.txt", "x");
    protect_with(&again, "D:P(A;;FA;;;SY)(A;;0x0;;;OW)").unwrap();
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around).unwrap();
    assert_eq!(
        asked(&around, 1),
        [verbatim(&matter.join("sub").join("x.txt"))]
    );
    assert!(!again.exists());
}

/// Around a temp matter whose credential link must be `credentials`; set-asides refused.
struct Credentials(PathBuf);

impl FolderList for Credentials {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Credentials {
    fn check(&self, _folder: &Path) -> io::Result<crate::windows::FolderSupport> {
        Ok(crate::windows::FolderSupport::Ok)
    }

    fn set_aside(&mut self, _matter: clauseo_protocol::MatterId, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

/// Round 4, row 5 of the second VM run (L5): the credential link, a hard link to `auth.json`,
/// gets the pool user's grant and the Low label. The agent then swaps the link for a hard link
/// to `other.json`: the next launch refuses, and forget took the entries back through the link,
/// so from `other.json`, leaving them on `auth.json`. Forget now takes them off `auth.json`,
/// the file the launch recorded.
#[test]
fn forget_takes_the_links_entries_off_auth_json_after_the_link_is_swapped() {
    let place = Place::new("apply-ntfs-link-swap");
    let auth = place.dir.file("creds/auth.json", "{}");
    let other = place.dir.file("creds/other.json", "{}");
    place.dir.hard_link("state/auth.json", &auth);
    let link = place.at("state/auth.json");
    let on_link = |sid: &str, rights| Grant {
        kind: PathKind::CredentialLink,
        ..grant(&link, sid, rights, Inherit::ThisOnly)
    };
    let grants = || {
        vec![
            on_link(USER_SID, Rights::ReadWriteData),
            on_link(LOW_LABEL_SID, Rights::LowLabel),
        ]
    };
    let mut around = Credentials(auth.clone());
    let (mut ledger, mut acl) = (ledger(&place), NtfsAcl::new().unwrap());
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants());
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();
    assert!(has(&auth, USER_SID, false) && has(&auth, LOW_LABEL_SID, false));

    crate::windows::write::rename_new(&link, &place.at("state/was-auth.json")).unwrap();
    place.dir.hard_link("state/auth.json", &other);
    let mut again = AdmittedPlan::from_grants(matter_one(), grants());
    let refused = apply(&mut ledger, &mut again, &mut acl, &mut around).unwrap_err();
    assert!(
        matches!(
            refused,
            Error::AclApply {
                path_kind: PathKind::CredentialLink,
                failure: ApplyFailure::Changed,
                ..
            }
        ),
        "{refused:?}"
    );

    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around).unwrap();
    assert_eq!((done.removed, done.kept), (2, 0));
    for sid in [USER_SID, LOW_LABEL_SID] {
        assert!(!has(&auth, sid, false), "{sid} left on auth.json");
        assert!(!has(&other, sid, false), "{sid} on other.json");
    }
    assert_eq!(ledger.entries().count(), 0);
}
