//! The later launch's timing on NTFS (ignored on CI; the VM runs it): a first launch, a session
//! end in PR6's order with the clean-close record written, then the later launch through the
//! launch check, against the full later launch. Trees of the perf design's deep shape: 10 files
//! per folder in three levels (`a*\b*\c*`) in the matter, 50 files in the state folder and 10
//! in the notes folder. Temp folders and synthetic SIDs. Run with
//! `--run-ignored only -E 'test(timing_later_launch)'`.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::write_clean_close;
use super::{Closed, LaunchCheck, LaunchPath, NtfsJournal, check_launch, session_cursor};
use crate::Policy;
use crate::windows::NtfsAcl;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, LOW_LABEL_SID, Rights};
use crate::windows::admit::{FolderList, PathKind, ReadDir};
use crate::windows::apply::{Surroundings, apply, apply_checked, repair_after_session};
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::set_aside::{after_session, before_launch, before_launch_checked};
use crate::windows::test_acl::grant;
use crate::windows::test_dacl::auto_inherit_with;
use crate::windows::test_world::{
    NOTES, OTHER_POOL_SIDS, STATE, USER_SID, World, matter_one, reserved, trusted,
};

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

/// A temp world whose matter holds `files` files in the deep shape, its state folder 50 and its
/// notes folder 10; and the matter's first file, for the edit.
fn deep_world(test: &str, files: usize) -> (World, PathBuf) {
    let w = World::new(test);
    let leaves = files.div_ceil(10);
    // The fan-out per level: the least whose cube holds every leaf folder.
    let fan = (1..=leaves.max(1))
        .find(|f: &usize| f.pow(3) >= leaves)
        .unwrap();
    for i in 0..files {
        let leaf = i / 10;
        let (a, b, c) = (leaf / (fan * fan), (leaf / fan) % fan, leaf % fan);
        w.dir
            .file(&format!("matter/a{a}/b{b}/c{c}/doc{i}.txt"), "x");
    }
    for i in 0..50 {
        w.dir.file(&format!("{STATE}/state{i}.json"), "{}");
    }
    for i in 0..10 {
        w.dir.file(&format!("{NOTES}/note{i}.md"), "n");
    }
    let first = w.canonical("matter/a0/b0/c0/doc0.txt");
    (w, first)
}

/// The pool user's `Modify`, another pool user's deny, and the label, on the matter, state and
/// notes folders.
fn plan(w: &World) -> AdmittedPlan {
    let mut grants = Vec::new();
    for (rel, kind) in [
        ("matter", PathKind::Matter),
        (STATE, PathKind::State),
        (NOTES, PathKind::Notes),
    ] {
        let root = w.canonical(rel);
        for (sid, rights) in [
            (USER_SID, Rights::Modify),
            (OTHER_POOL_SIDS[0], Rights::DenyReadWrite),
            (LOW_LABEL_SID, Rights::LowLabel),
        ] {
            grants.push(Grant {
                kind,
                ..grant(&root, sid, rights, Inherit::Tree)
            });
        }
    }
    AdmittedPlan::from_grants(matter_one(), grants)
}

fn ledger(data: &Path) -> Ledger {
    Ledger::open(ledger_path(data), &trusted(), &reserved()).unwrap()
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_762_337_700 + seconds)
}

fn ms(elapsed: Duration) -> u128 {
    elapsed.as_millis()
}

/// The first launch and a clean session end on `w`, then `edit` (if any), then the later
/// launch through the check, then the full later launch; prints one line and returns the
/// check's path.
#[expect(
    clippy::disallowed_methods,
    reason = "times the launch; the core never reads a clock"
)]
fn later_launch(w: &World, files: usize, edit: Option<&Path>) -> LaunchPath {
    let data = w.canonical("data");
    let policy: Policy = w.builder().build().unwrap();
    let matter = matter_one();
    let acl = || NtfsAcl::new().unwrap();

    // The first launch.
    before_launch(&policy, &data, &matter, "s1", &[], at(0)).unwrap();
    apply(&mut ledger(&data), &mut plan(w), &mut acl(), &mut Around).unwrap();

    // The session end, in PR6's order.
    let closing = ledger(&data);
    let cursor = session_cursor(&closing, matter, &NtfsJournal);
    assert!(
        cursor.is_some(),
        "no cursor: no journal, or a root not NTFS"
    );
    let repair = repair_after_session(&closing, matter, &mut acl(), &mut Around);
    let after = after_session(&policy, &data, &matter, "s1", at(60));
    let closed = write_clean_close(
        &data,
        &ledger(&data),
        matter,
        &NtfsJournal,
        cursor.as_ref(),
        &repair,
        &after,
    );
    assert_eq!(closed, Closed::Written);

    if let Some(path) = edit {
        // An Administrators grant: the strict walk accepts it (a broad Everyone allow it
        // rightly refuses), so the launch is targeted and passes.
        auto_inherit_with(path, "D:(A;;FA;;;BA)").unwrap();
    }

    // The later launch: the check, the set-aside's launch pass, then apply, all with the check.
    let started = Instant::now();
    let check: LaunchCheck = check_launch(&data, &ledger(&data), &plan(w), &NtfsJournal).unwrap();
    let checked = started.elapsed();
    let started = Instant::now();
    before_launch_checked(&policy, &data, &matter, "s2", &[], at(120), &check).unwrap();
    let before = started.elapsed();
    let started = Instant::now();
    apply_checked(
        &mut ledger(&data),
        &mut plan(w),
        &mut acl(),
        &mut Around,
        &check,
    )
    .unwrap();
    let applied = started.elapsed();
    let total = checked + before + applied;

    // The full later launch, for comparison: the full set-aside pass and the full walk.
    let started = Instant::now();
    before_launch(&policy, &data, &matter, "s3", &["s2"], at(180)).unwrap();
    apply(&mut ledger(&data), &mut plan(w), &mut acl(), &mut Around).unwrap();
    let full = started.elapsed();

    println!(
        "files {files}: path {:?}, check {} ms, apply_checked {} ms, before_launch {} ms, \
         total {} ms; full later launch (before_launch + apply) {} ms",
        check.path(),
        ms(checked),
        ms(applied),
        ms(before),
        ms(total),
        ms(full),
    );
    check.path()
}

#[test]
#[ignore = "S15 PR3d timing row; makes up to 50,000 files"]
fn timing_later_launch_with_nothing_changed() {
    for files in [1_000, 5_000, 20_000, 50_000] {
        let (w, _) = deep_world(&format!("journal-timing-{files}"), files);
        let path = later_launch(&w, files, None);
        assert_eq!(path, LaunchPath::Skipped, "files {files}");
    }
}

#[test]
#[ignore = "S15 PR3d timing row; makes 5,000 files"]
fn timing_later_launch_after_one_edit() {
    let files = 5_000;
    let (w, first) = deep_world("journal-timing-edit", files);
    let path = later_launch(&w, files, Some(&first));
    assert!(matches!(path, LaunchPath::Targeted(_)), "{path:?}");
}
