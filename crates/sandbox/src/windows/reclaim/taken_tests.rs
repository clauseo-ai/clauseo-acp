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
