//! The later launch's timing on NTFS (ignored on CI; the VM runs it): a first launch, a session
//! end in PR6's order with the clean-close record written, then the later launch through the
//! launch check, against the full later launch. Trees of the perf design's deep shape: 10 files
//! per folder in three levels (`a*\b*\c*`) in the matter, 50 files in the state folder and 10
//! in the notes folder. Temp folders and synthetic SIDs. The plan holds the policy's read-only
//! denies too, and the later launch's cost includes the policy's walk of the matter, which runs
//! at every launch and finds the nested protected names (S15 `PR4a2`). Run with
//! `--run-ignored only -E 'test(timing_later_launch)'`, and `CLAUSEO_WALK_FILES=50000` for the
//! nested row's size.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::write_clean_close;
use super::{Closed, LaunchCheck, LaunchPath, NtfsJournal, check_launch, session_cursor};
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
use crate::{PathRule, Policy};

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

/// How many nested protected items the nested row's matter holds.
const NESTED: usize = 50;

/// Leaf folder `leaf` of a deep-shaped matter of `files` files, relative to the temp folder.
fn leaf_folder(files: usize, leaf: usize) -> String {
    let leaves = files.div_ceil(10);
    // The fan-out per level: the least whose cube holds every leaf folder.
    let fan = (1..=leaves.max(1))
        .find(|f: &usize| f.pow(3) >= leaves)
        .unwrap();
    let (a, b, c) = (leaf / (fan * fan), (leaf / fan) % fan, leaf % fan);
    format!("matter/a{a}/b{b}/c{c}")
}

/// A temp world whose matter holds `files` files in the deep shape, its state folder 50 and its
/// notes folder 10; and the matter's first file, for the edit.
fn deep_world(test: &str, files: usize) -> (World, PathBuf) {
    let w = World::new(test);
    for i in 0..files {
        let folder = leaf_folder(files, i / 10);
        w.dir.file(&format!("{folder}/doc{i}.txt"), "x");
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
/// notes folders; and the pool user's deny of writes on each of `policy`'s read-only paths in
/// the matter, as `acl_plan` gives it.
fn plan(w: &World, policy: &Policy) -> AdmittedPlan {
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
    for rule in policy.read_only_in_matter() {
        let inherit = match rule {
            PathRule::Literal(_) => Inherit::ThisOnly,
            PathRule::Subpath(_) => Inherit::Tree,
        };
        grants.push(Grant {
            kind: PathKind::ReadOnlyInMatter,
            ..grant(rule.path(), USER_SID, Rights::DenyWrite, inherit)
        });
    }
    AdmittedPlan::from_grants(matter_one(), grants)
}

/// How many of `policy`'s read-only paths exist: the denies a launch writes.
fn denies(policy: &Policy) -> usize {
    policy
        .read_only_in_matter()
        .iter()
        .filter(|rule| rule.path().exists())
        .count()
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
/// launch through the check (the policy's walk first), then the full later launch; prints one
/// line and returns the check's path.
#[expect(
    clippy::disallowed_methods,
    reason = "times the launch; the core never reads a clock"
)]
fn later_launch(w: &World, files: usize, edit: Option<&dyn Fn()>) -> LaunchPath {
    let data = w.canonical("data");
    let matter = matter_one();
    let acl = || NtfsAcl::new().unwrap();

    // The first launch: the policy's walk, the set-aside's pass, then apply.
    let started = Instant::now();
    let policy: Policy = w.builder().build().unwrap();
    let first_walk = started.elapsed();
    before_launch(&policy, &data, &matter, "s1", &[], at(0)).unwrap();
    apply(
        &mut ledger(&data),
        &mut plan(w, &policy),
        &mut acl(),
        &mut Around,
    )
    .unwrap();
    let first = started.elapsed();

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

    if let Some(edit) = edit {
        edit();
    }

    // The later launch: the policy's walk, which runs whatever the check decides, then the
    // check, the set-aside's launch pass and apply, all with the check.
    let started = Instant::now();
    let policy: Policy = w.builder().build().unwrap();
    let walked = started.elapsed();
    let later = plan(w, &policy);
    let started = Instant::now();
    let check: LaunchCheck = check_launch(&data, &ledger(&data), &later, &NtfsJournal).unwrap();
    let checked = started.elapsed();
    let started = Instant::now();
    before_launch_checked(&policy, &data, &matter, "s2", &[], at(120), &check).unwrap();
    let before = started.elapsed();
    let started = Instant::now();
    apply_checked(
        &mut ledger(&data),
        &mut plan(w, &policy),
        &mut acl(),
        &mut Around,
        &check,
    )
    .unwrap();
    let applied = started.elapsed();
    let total = walked + checked + before + applied;

    // The full later launch, for comparison: the full set-aside pass and the full walk.
    let started = Instant::now();
    before_launch(&policy, &data, &matter, "s3", &["s2"], at(180)).unwrap();
    apply(
        &mut ledger(&data),
        &mut plan(w, &policy),
        &mut acl(),
        &mut Around,
    )
    .unwrap();
    let full = started.elapsed();

    println!(
        "files {files}: first launch {} ms (policy walk {} ms); later: path {:?}, read-only \
         denies {}, policy walk {} ms, check {} ms, apply_checked {} ms, before_launch {} ms, \
         total {} ms; full later launch (policy walk + before_launch + apply) {} ms",
        ms(first),
        ms(first_walk),
        check.path(),
        denies(&policy),
        ms(walked),
        ms(checked),
        ms(applied),
        ms(before),
        ms(total),
        ms(walked + full),
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
    // An Administrators grant: the strict walk accepts it (a broad Everyone allow it rightly
    // refuses), so the launch is targeted and passes.
    let edit = || auto_inherit_with(&first, "D:(A;;FA;;;BA)").unwrap();
    let path = later_launch(&w, files, Some(&edit));
    assert!(matches!(path, LaunchPath::Targeted(_)), "{path:?}");
}

/// [`deep_world`] with [`NESTED`] nested protected items spread over its leaf folders, in turn
/// a `CLAUDE.md`, a `.git` holding a `config`, and a `.claude` holding a `settings.json`.
fn nested_world(test: &str, files: usize) -> World {
    let (w, _) = deep_world(test, files);
    let leaves = files.div_ceil(10);
    for k in 0..NESTED {
        let folder = leaf_folder(files, k * leaves / NESTED);
        let item = match k % 3 {
            0 => "CLAUDE.md",
            1 => ".git/config",
            _ => ".claude/settings.json",
        };
        w.dir.file(&format!("{folder}/{item}"), "x");
    }
    w
}

/// `CLAUSEO_WALK_FILES` (50,000 by default).
fn walk_files() -> usize {
    std::env::var("CLAUSEO_WALK_FILES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50_000)
}

/// S15 `PR4a2`'s rows: `CLAUSEO_WALK_FILES` files with [`NESTED`] nested protected items, each
/// write-denied. The later launch with nothing changed, then with a new nested `CLAUDE.md` the
/// lawyer made between sessions (targeted, its deny written), each against the full one.
#[test]
#[ignore = "S15 PR4a2 timing row; makes 50,000 files twice"]
fn timing_later_launch_with_nested_items() {
    let files = walk_files();
    let w = nested_world("journal-timing-nested", files);
    let path = later_launch(&w, files, None);
    assert_eq!(path, LaunchPath::Skipped);
    let w = nested_world("journal-timing-nested-new", files);
    let made = format!("{}/CLAUDE.md", leaf_folder(files, 1));
    let edit = || {
        w.dir.file(&made, "# the lawyer's, new\r\n");
    };
    let path = later_launch(&w, files, Some(&edit));
    assert!(matches!(path, LaunchPath::Targeted(_)), "{path:?}");
}
