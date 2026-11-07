//! The items `--reclaim` took, end to end on every OS: the exe's list (`reclaim-taken.json`)
//! read back, recorded in the ledger as `Taken`, each a root alone, handed to the strip as the
//! only items that lose denies (with the pool and its group kept), and cleared once a hand-back
//! refuses nothing; one a refused hand-back leaves stays for the next.

use serde_json::json;

use super::tests::{Hands, abs, ledger_with, rows};
use super::*;
use crate::test_support::TempDir;
use crate::windows::ledger::ledger_path;
use crate::windows::setup::taken_file;
use crate::windows::test_world::{USER_SID, matter_one, reserved, trusted};

/// `run`'s answer, and how long it took on the wall clock.
#[expect(
    clippy::disallowed_methods,
    reason = "a test checks a bound on wall-clock time"
)]
fn timed<T>(run: impl FnOnce() -> T) -> (T, std::time::Duration) {
    let started = std::time::Instant::now();
    let answer = run();
    (answer, started.elapsed())
}

fn item(name: &str, id: u8) -> TakenItem {
    TakenItem {
        matter: matter_one(),
        path: abs(name),
        file: FileId {
            volume: 7,
            id: [id; 16],
        },
    }
}

fn taken_owed(ledger: &Ledger) -> Vec<(PathBuf, Option<FileId>)> {
    let owed = ledger.reclaims_owed().into_iter();
    owed.filter(|o| o.why == Stuck::Taken)
        .map(|o| (o.path, o.file))
        .collect()
}

#[test]
fn the_exes_list_is_recorded_stripped_with_the_pool_kept_and_cleared() {
    let temp = TempDir::new("reclaim-taken-trip");
    let mut ledger = ledger_with(&temp, &json!({}));
    let doc = item("m1/doc.txt", 1);
    taken_file::write(temp.path(), std::slice::from_ref(&doc)).unwrap();
    let read = taken_file::read(temp.path()).unwrap();
    assert_eq!(
        read,
        std::slice::from_ref(&doc),
        "the exe's list reads back"
    );
    let pool = [Sid::parse(USER_SID).unwrap()];
    let group = Sid::parse("S-1-5-21-9-9-9-1600").unwrap();
    let kept = [pool[0].clone(), group];
    let mut acl = Hands::default();

    let handed = hand_back_after(&mut acl, &mut ledger, &rows(), &read, &pool, &kept).unwrap();

    assert_eq!(handed.refused, []);
    assert_eq!(
        acl.roots,
        Vec::<PathBuf>::new(),
        "a taken root: no owner hand-back"
    );
    let stripped: Vec<(PathBuf, bool)> = acl.strips.iter().map(|s| (s.0.clone(), s.3)).collect();
    assert_eq!(
        stripped,
        [(abs("m1/doc.txt"), false)],
        "the strip alone, root alone"
    );
    assert_eq!(acl.denies, [(vec![(doc.path, doc.file)], kept.to_vec())]);
    assert_eq!(taken_owed(&ledger), [], "cleared once nothing was refused");
    let reread = Ledger::open(ledger_path(temp.path()), &trusted(), &reserved()).unwrap();
    assert_eq!(taken_owed(&reread), [], "and on disk");
    taken_file::remove(temp.path()).unwrap();
    assert_eq!(taken_file::read(temp.path()), Ok(Vec::new()));
}

/// A taken item inside a tree walked whole is no target of its own; the tree's strip is given it.
/// When that hand-back is refused the record stays, and the next hand-back, which refuses
/// nothing, strips it again from the ledger alone and clears it.
#[test]
fn a_refused_hand_back_keeps_the_taken_item_for_the_next() {
    let temp = TempDir::new("reclaim-taken-refused");
    let owed = json!([{ "matter": matter_one().to_string(), "pool": USER_SID }]);
    let mut ledger = ledger_with(&temp, &json!({ "handbacks": owed }));
    let doc = item("m1/work/doc.txt", 2);
    let pool = [Sid::parse(USER_SID).unwrap()];
    let mut refusing = Hands {
        shut: Some(abs("m1")),
        ..Hands::default()
    };

    let first = hand_back_after(
        &mut refusing,
        &mut ledger,
        &rows(),
        std::slice::from_ref(&doc),
        &pool,
        &pool,
    )
    .unwrap();

    assert_eq!(first.refused.len(), 1);
    let m1 = refusing
        .strips
        .iter()
        .position(|s| s.0 == abs("m1"))
        .unwrap();
    assert_eq!(refusing.denies[m1].0, [(doc.path.clone(), doc.file)]);
    assert!(
        !refusing.strips.iter().any(|s| s.0 == doc.path),
        "covered by m1"
    );
    assert_eq!(taken_owed(&ledger), [(doc.path.clone(), Some(doc.file))]);

    let mut acl = Hands::default();
    let again = hand_back_after(&mut acl, &mut ledger, &rows(), &[], &pool, &pool).unwrap();

    assert_eq!(again.refused, []);
    let m1 = acl.strips.iter().position(|s| s.0 == abs("m1")).unwrap();
    assert_eq!(
        acl.denies[m1].0,
        [(doc.path, doc.file)],
        "from the ledger alone"
    );
    assert_eq!(taken_owed(&ledger), []);
}

/// What the strip is given lists only what was taken: a matter's own tree (an owed hand-back)
/// with nothing taken gives the strip no item, so no deny on it goes.
#[test]
fn nothing_taken_gives_the_strip_no_item() {
    let temp = TempDir::new("reclaim-taken-none");
    let owed = json!([{ "matter": matter_one().to_string(), "pool": USER_SID }]);
    let mut ledger = ledger_with(&temp, &json!({ "handbacks": owed }));
    let pool = [Sid::parse(USER_SID).unwrap()];
    let mut acl = Hands::default();

    hand_back_after(&mut acl, &mut ledger, &rows(), &[], &pool, &pool).unwrap();

    assert_eq!(acl.strips.len(), 3, "m1, n1 and s1");
    assert!(acl.denies.iter().all(|(taken, _)| taken.is_empty()));
    assert_eq!(acl.other, 0);
}

/// The exe's side: what one target's walk took goes into the list under that target's matter,
/// in the walk's order; a path that isn't text is left out, as the list can't carry it.
#[test]
fn the_exe_lists_what_a_targets_walk_took_under_its_matter() {
    let (a, b) = (item("m1/a.txt", 3), item("m1/b.txt", 4));
    let drained = vec![(a.path.clone(), a.file), (b.path.clone(), b.file)];
    #[cfg(unix)]
    let drained = {
        use std::os::unix::ffi::OsStrExt as _;
        let odd = std::ffi::OsStr::from_bytes(b"/tmp/\xff.txt");
        let mut with_odd = drained;
        with_odd.push((PathBuf::from(odd), a.file));
        with_odd
    };

    let listed = taken_items(matter_one(), drained);

    assert_eq!(listed, [a, b]);
}

/// A list that can't be read adds nothing: corrupt, or larger than the cap. The hand-back then
/// strips denies on what the ledger already lists, and nothing more.
#[test]
fn a_corrupt_or_oversized_list_adds_nothing() {
    let temp = TempDir::new("reclaim-taken-bad");
    let corrupt = temp.file("sandbox/reclaim-taken.json", "{ not json");
    assert_eq!(corrupt, taken_file::path(temp.path()));
    assert!(taken_file::read(temp.path()).is_err(), "corrupt");
    taken_file::remove(temp.path()).unwrap();
    let big = "x".repeat(16 * 1024 * 1024 + 1);
    temp.file("sandbox/reclaim-taken.json", &big);
    assert!(taken_file::read(temp.path()).is_err(), "over the cap");
}

/// Many items, each looked up as the strip looks them up: ten thousand recorded, indexed and
/// each found, and cleared, in well under a second.
#[test]
fn ten_thousand_taken_items_are_recorded_indexed_and_cleared_quickly() {
    let temp = TempDir::new("reclaim-taken-many");
    let mut ledger = ledger_with(&temp, &json!({}));
    let many: Vec<(MatterId, PathBuf, FileId)> = (0..10_000_u32)
        .map(|n| {
            let mut id = [0; 16];
            id[..4].copy_from_slice(&n.to_le_bytes());
            let path = abs(&format!("m1/many/{n}.txt"));
            (matter_one(), path, FileId { volume: 7, id })
        })
        .collect();

    let ((found, owed), took) = timed(|| {
        owe_taken(&mut ledger, &many).unwrap();
        let index = TakenIndex::new(many.iter().map(|(_, p, f)| (p.clone(), *f)));
        let found = many
            .iter()
            .filter(|(_, p, f)| index.ids(p).contains(f))
            .count();
        let owed: Vec<ReclaimOwed> = ledger.reclaims_owed();
        clear_taken(&mut ledger, &owed).unwrap();
        (found, owed)
    });

    assert_eq!(found, 10_000);
    assert_eq!(owed.len(), 10_000);
    assert_eq!(taken_owed(&ledger), []);
    assert!(took < std::time::Duration::from_secs(1), "{took:?}");
}
