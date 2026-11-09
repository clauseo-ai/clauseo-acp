//! The capture's cost with many matters (#764 round 3 review): 500 matters under one folder are
//! captured with each name on the disk looked at once and each identity asked once, counted on
//! the fakes, not timed; and a check asks at most 24 missing places whether they are there now,
//! the rest in turn, so a place made late in another matter is still seen.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{FakeIds, capture_on, matter, rules};
use crate::actor::turn::denials::offer::identity::FileId;
use crate::actor::turn::denials::offer::{KnownMatter, OfferRules};
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::plain_net;

const MATTERS: u128 = 500;

/// [`rules`] with 500 more matters under `/srv/matters`.
fn many() -> OfferRules {
    let mut rules = rules();
    rules.matters.extend((0..MATTERS).map(|n| KnownMatter {
        id: matter(100 + n),
        name: format!("M{n}"),
        root: format!("/srv/matters/M{n}").into(),
    }));
    rules
}

/// A disk with home, the data folder and every matter's root, and an identity for each folder
/// on it.
fn disk_and_ids(rules: &OfferRules) -> (FakeFs, FakeIds) {
    let mut names: Vec<(String, Fake)> = vec![
        ("/home/a/.local/share/clauseo".into(), Fake::Dir),
        ("/home/a/Matters/Acme".into(), Fake::Dir),
    ];
    for known in &rules.matters {
        names.push((known.root.display().to_string(), Fake::Dir));
    }
    let named: Vec<(&str, Fake)> = names.iter().map(|(p, f)| (p.as_str(), f.clone())).collect();
    let fs = FakeFs::with(&named);
    let mut ids = FakeIds::default();
    for (n, path) in (1_u128..).zip(fs.names.keys()) {
        ids.ids.insert(path.clone(), FileId { volume: 1, file: n });
    }
    (fs, ids)
}

#[test]
fn many_matters_are_captured_with_each_name_looked_at_once() {
    let rules = many();
    let (fs, ids) = disk_and_ids(&rules);
    let captured = capture_on(rules, &ids, &fs, &plain_net(false));
    // Each matter's root and its five dot folders, home's places, and the folders above.
    let bound = usize::try_from(MATTERS).unwrap() * 6 + 100;
    let touched = fs.touched();
    let once: HashSet<&PathBuf> = touched.iter().collect();
    assert_eq!(once.len(), touched.len(), "a name looked at twice");
    assert!(touched.len() <= bound, "{} looks", touched.len());
    let asked = ids.asked();
    let once: HashSet<&PathBuf> = asked.iter().collect();
    assert_eq!(once.len(), asked.len(), "an identity asked twice");
    assert!(asked.len() <= bound, "{} identities", asked.len());
    assert!(captured.places.len() > usize::try_from(MATTERS).unwrap());
}

#[test]
fn a_check_asks_a_bounded_number_of_missing_places_and_the_rest_in_turn() {
    let rules = many();
    let (fs, early) = disk_and_ids(&rules);
    let captured = capture_on(rules, &early, &fs, &plain_net(false));
    let before = early.asked().len();
    assert!(!captured.places.stale(&early));
    let asked = early.asked().len() - before;
    assert!((1..=24).contains(&asked), "{asked} asked in one check");
    // `.claude` made in the last matter after the capture: seen within one turn through them.
    let (_, mut late) = disk_and_ids(&many());
    let made = Path::new("/srv/matters/M499/.claude");
    late.ids.insert(made.into(), FileId { volume: 1, file: 0 });
    let turns = usize::try_from(MATTERS).unwrap() * 5 / 8 + 2;
    let seen = (0..turns).position(|_| captured.places.stale(&late));
    assert!(seen.is_some(), "not seen in {turns} checks");
}
