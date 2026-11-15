//! The matter's link walk (`matter_links.rs`): a link out of the matter, a broken one, a walk
//! past its cap or its time limit, an entry it can't read, or a matter that can't be listed
//! leave the matter without read allow rules; a link that stays inside, or none, keeps them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use clauseo_protocol::{MatterId, Timestamp};

use super::{
    Found, Kind, Listing, WALK_BUDGET, WalkFs, counts_as_link, links_out, readable_matter, walk,
    walk_within,
};
use crate::ManualClock;
use crate::pipeline::tests::support::TempDir;

/// A matter under `dir` with a file and a sub-folder, and a folder beside it.
fn matter(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let matter = dir.path().join("Smith v Jones (2024)");
    let outside = dir.path().join("data");
    for folder in [matter.join("sub"), outside.clone()] {
        crate::write::create_dir_all(&folder).unwrap();
    }
    crate::write::write_file(&matter.join("sub").join("memo.txt"), b"memo").unwrap();
    crate::write::write_file(&outside.join("settings.json"), b"{}").unwrap();
    (dunce::canonicalize(&matter).unwrap(), outside)
}

/// A plain matter keeps its rules, in both spellings when they differ.
#[test]
fn a_matter_with_no_link_keeps_its_spellings() {
    let dir = TempDir::new("matter-links-none");
    let (real, _) = matter(&dir);
    assert_eq!(links_out(&real, 100), Some(false));
    assert_eq!(readable_matter(&real, &real), std::slice::from_ref(&real));
    let stored = dir.path().join("stored");
    assert_eq!(readable_matter(&real, &stored), [real, stored]);
}

/// A matter that can't be listed, or one with more entries than the cap, gets none.
#[test]
fn a_matter_past_the_cap_or_unlistable_gets_no_rules() {
    let dir = TempDir::new("matter-links-cap");
    let (real, _) = matter(&dir);
    for n in 0..5 {
        crate::write::write_file(&real.join(format!("f{n}.txt")), b"x").unwrap();
    }
    assert_eq!(links_out(&real, 4), None);
    assert_eq!(links_out(&real, 100), Some(false));
    let gone = dir.path().join("gone");
    assert_eq!(links_out(&gone, 100), None);
    assert_eq!(
        readable_matter(&gone, &gone),
        Vec::<std::path::PathBuf>::new()
    );
}

/// A symbolic link out of the matter (to a folder or a file, at any depth) or a broken one
/// leaves it without rules; one that stays inside does not (the review's H2).
#[cfg(unix)]
#[test]
fn a_symbolic_link_out_of_the_matter_leaves_it_without_rules() {
    for (name, out) in [
        ("dir out", true),
        ("file out", true),
        ("broken", true),
        ("inside", false),
    ] {
        let dir = TempDir::new("matter-links-unix");
        let (real, outside) = matter(&dir);
        let (target, link) = match name {
            "dir out" => (outside, real.join("link")),
            "file out" => (outside.join("settings.json"), real.join("sub").join("f")),
            "broken" => (real.join("nothing"), real.join("sub").join("dangling")),
            _ => (real.join("sub"), real.join("again")),
        };
        crate::write::symlink(&target, &link).unwrap();
        assert_eq!(links_out(&real, 100), Some(out), "{name}");
        assert_eq!(readable_matter(&real, &real).is_empty(), out, "{name}");
    }
}

/// A junction out of the matter leaves it without rules.
#[cfg(windows)]
#[test]
fn a_junction_out_of_the_matter_leaves_it_without_rules() {
    let dir = TempDir::new("matter-links-junction");
    let (real, outside) = matter(&dir);
    clauseo_sandbox::windows::test_junction::junction(&real.join("link"), &outside).unwrap();
    assert_eq!(links_out(&real, 100), Some(true));
    assert_eq!(
        readable_matter(&real, &real),
        Vec::<std::path::PathBuf>::new()
    );
}

/// The tag decision, on every system: a symbolic link, a junction (mount point) and a kind not
/// known to be local count as links; `OneDrive`'s cloud files (every variant), dedup, WOF and
/// WCI are data, so a matter in `OneDrive` keeps its rules.
#[test]
fn only_links_and_unknown_reparse_points_count_as_links() {
    for tag in [0xA000_000C, 0xA000_0003, 0x8000_000A, 0x8000_001E, 0x1234] {
        assert!(counts_as_link(tag), "{tag:#x}");
    }
    for tag in [
        0x9000_001A,
        0x9000_101A,
        0x9000_F01A,
        0x8000_0013,
        0x8000_0017,
        0x8000_0018,
    ] {
        assert!(!counts_as_link(tag), "{tag:#x}");
    }
}

/// A matter nested inside the session's matter, past #1085's 2,000-path fence cap (2,001
/// entries sit beside it), leaves the session's matter with no rules at all; the session matter's
/// own `.clauseo` does not. #1085's `denies_inside` answers for the nested matters its fence
/// listed; this holds for the rest, with the stand-in and with the real method.
#[test]
fn a_matter_nested_past_the_fence_cap_leaves_the_matter_without_rules() {
    use crate::agent::data_fence::{LaunchDeny, matter_shell_allow_rules};
    let dir = TempDir::new("matter-links-nested");
    let (real, _) = matter(&dir);
    let marker = |folder: &Path| {
        crate::write::create_dir_all(&folder.join(".clauseo")).unwrap();
        crate::write::write_file(&folder.join(".clauseo").join("matter.json"), b"{}").unwrap();
    };
    marker(&real);
    let bulk = real.join("bulk");
    crate::write::create_dir_all(&bulk).unwrap();
    for n in 0..2_001 {
        crate::write::write_file(&bulk.join(format!("f{n}.txt")), b"x").unwrap();
    }
    let mut deny = LaunchDeny::fixed(vec![dir.path().join("clauseo-data")]);
    deny.matter = readable_matter(&real, &real);
    assert_eq!(
        deny.matter,
        std::slice::from_ref(&real),
        "its own marker keeps the rules"
    );
    assert_ne!(matter_shell_allow_rules(&deny, false), Vec::<String>::new());

    marker(&bulk.join("Appeal (2025)"));
    assert_eq!(links_out(&real, super::SCAN_CAP), Some(true));
    deny.matter = readable_matter(&real, &real);
    assert_eq!(deny.matter, Vec::<PathBuf>::new());
    assert_eq!(matter_shell_allow_rules(&deny, false), Vec::<String>::new());
}

/// A stand-in file system: `list` gives each folder's entries, and every path resolves to
/// itself.
struct Fake<L>(L);

impl<L: Fn(&Path) -> io::Result<Vec<io::Result<Found>>>> WalkFs for Fake<L> {
    fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_owned())
    }

    fn list(&self, folder: &Path) -> io::Result<Listing<'_>> {
        Ok(Box::new((self.0)(folder)?.into_iter()))
    }
}

fn found(path: PathBuf, kind: io::Result<Kind>) -> Found {
    Found { path, kind }
}

/// A relative matter path no folder is at, so `agent::session_folder` gives it back as it is.
const NOWHERE: &str = "no-such-matter-folder-for-the-walk";

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

fn matter_id() -> MatterId {
    MatterId::new(uuid::Uuid::now_v7()).unwrap()
}

/// The walk fails closed: an entry the listing could not read, or one whose kind could not be
/// read, gives no answer (so no rules) where the same folder without it gives `Some(false)`.
#[test]
fn an_entry_or_a_kind_the_walk_cannot_read_gives_no_rules() {
    let root = Path::new("matter");
    let never = || false;
    for (name, bad, want) in [
        ("plain", None, Some(false)),
        ("entry", Some(0), None),
        ("kind", Some(1), None),
    ] {
        let fs = Fake(|folder: &Path| {
            let mut entries = vec![Ok(found(folder.join("memo.txt"), Ok(Kind::File)))];
            if folder == root {
                entries.push(Ok(found(root.join("sub"), Ok(Kind::Folder))));
                match bad {
                    Some(0) => entries.push(Err(io::Error::other("unreadable entry"))),
                    Some(_) => {
                        entries.push(Ok(found(root.join("x"), Err(io::Error::other("kind")))))
                    }
                    None => {}
                }
            }
            Ok(entries)
        });
        assert_eq!(walk(&fs, root, 100, &never), want, "{name}");
        let kept = super::readable_in(&fs, root, root, &never);
        assert_eq!(kept.is_empty(), want.is_none(), "{name}");
    }
}

/// A walk slower than [`WALK_BUDGET`] (each listing takes a second on the injected clock, and
/// the folders never end) stops itself after its second listing and gives no rules: `lists`
/// ends once the walk's thread has dropped the stand-in.
#[test]
fn a_walk_past_its_time_limit_stops_and_gives_no_rules() {
    let clock = clock();
    let (listed, lists) = mpsc::channel::<()>();
    let ticking = clock.clone();
    let fs = Fake(move |folder: &Path| {
        listed.send(()).unwrap();
        ticking.advance(std::time::Duration::from_secs(1));
        Ok(vec![Ok(found(folder.join("deeper"), Ok(Kind::Folder)))])
    });
    let walked = walk_within(Arc::new(clock), matter_id(), fs, PathBuf::from(NOWHERE));
    assert_eq!(futures::executor::block_on(walked), Vec::<PathBuf>::new());
    assert_eq!(lists.iter().count(), 2, "the walk stopped at the budget");
}

/// A listing the disk never answers (a mapped drive that hangs): the wait gives up at
/// [`WALK_BUDGET`] on the injected clock and the matter gets no rules, while the listing is
/// still blocked.
#[test]
fn a_walk_the_disk_never_answers_gives_no_rules_at_the_budget() {
    let clock = clock();
    let (entered, started) = async_channel::unbounded::<()>();
    let (release, blocked) = mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let fs = Fake(move |_: &Path| {
        let _told = entered.send_blocking(());
        let _released = blocked.lock().map(|blocked| blocked.recv());
        Ok(Vec::new())
    });
    let walked = walk_within(Arc::new(clock.clone()), matter_id(), fs, NOWHERE.into());
    let give_up = async move {
        started.recv().await.unwrap();
        assert_eq!(clock.next_armed().await, WALK_BUDGET);
        clock.advance(WALK_BUDGET);
    };
    let (found, ()) = futures::executor::block_on(futures::future::join(walked, give_up));
    assert_eq!(found, Vec::<PathBuf>::new());
    drop(release);
}

/// The same seam with a quick walk keeps the rules: the budget alone takes nothing away.
#[test]
fn a_quick_walk_keeps_the_rules() {
    let fs = Fake(|folder: &Path| Ok(vec![Ok(found(folder.join("memo.txt"), Ok(Kind::File)))]));
    let walked = walk_within(Arc::new(clock()), matter_id(), fs, NOWHERE.into());
    assert_eq!(
        futures::executor::block_on(walked),
        [PathBuf::from(NOWHERE)]
    );
}
