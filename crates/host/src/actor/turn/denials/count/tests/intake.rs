//! The intake's guards: the sliding window and the uncounted lane, the checks in flight, a
//! dropped check counted, tool calls seen running, the watched path's length, the dedupe by
//! identity and by folded text, a card not offered replaced by an offered one, one count per
//! refusal however many sources report it, and the OS confirmation by identity.

use std::time::Duration;

use clauseo_protocol::{DenialKind, ThreadId, TurnId};

use super::{MS, Minter, call, denial, folder, thread, turn};
use crate::actor::turn::denials::count::{
    DENIALS_PER_WINDOW, Denials, MAX_IN_FLIGHT, MAX_PATH_BYTES, Rate, Record, Recorded,
};
use crate::actor::turn::denials::offer::identity::{FileId, Target};

fn id(n: u128) -> FileId {
    FileId { volume: 1, file: n }
}

fn target(text: &str, id: Option<FileId>) -> Target {
    Target {
        id,
        text: text.to_owned(),
    }
}

/// Records `kind` about `card`, refused at `asked`.
fn put(
    denials: &mut Denials,
    (t, tu): (ThreadId, TurnId),
    (kind, offered, os): (&DenialKind, bool, bool),
    (card, asked): (&Target, &Target),
    minter: &mut Minter,
) -> (Recorded, Option<(TurnId, u16)>) {
    let record = Record {
        kind,
        offered,
        card,
        asked: Some(asked),
        os,
    };
    denials
        .record(t, Some(tu), Some(tu), record, minter.mint())
        .unwrap()
}

#[test]
fn the_window_slides_and_uncounted_denials_have_their_own() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), Some(turn(1)));
    let edge = Duration::from_millis(999);
    let passed = (0..DENIALS_PER_WINDOW)
        .filter(|_| denials.admit(t, tu, edge, 0, Rate::Counted).pass)
        .count();
    assert_eq!(passed, DENIALS_PER_WINDOW as usize);
    // Past a fixed window's edge a burst of as many again passed; a sliding one holds them.
    let after_edge = Duration::from_millis(1001);
    assert!(!denials.admit(t, tu, after_edge, 0, Rate::Counted).pass);
    // `ReadOnly` and an upstream sign-in never use up the counted window, nor it theirs.
    assert!(denials.admit(t, tu, after_edge, 0, Rate::Uncounted).pass);
    let later = edge + Duration::from_secs(1);
    assert!(denials.admit(t, tu, later, 0, Rate::Counted).pass);
}

#[test]
fn at_most_two_checks_run_at_once_and_a_dropped_one_counts() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    let held: Vec<_> = (0..MAX_IN_FLIGHT)
        .map(|_| denials.start_check(t).unwrap())
        .collect();
    assert!(denials.start_check(t).is_none());
    assert_eq!(denials.in_flight(t), MAX_IN_FLIGHT);
    // A thread the host no longer has is kept while its checks run.
    denials.prune(|_| false);
    assert_eq!(denials.in_flight(t), MAX_IN_FLIGHT);
    drop(held);
    assert_eq!(denials.in_flight(t), 0);
    assert!(denials.start_check(t).is_some());
    denials.prune(|_| false);
    // Three dropped checks pause the turn, as a `Dropped` would.
    assert_eq!(denials.dropped_check(t, Some(tu)), None);
    assert_eq!(denials.dropped_check(t, Some(tu)), None);
    assert_eq!(denials.dropped_check(t, Some(tu)), Some((tu, 3)));
}

#[test]
fn only_a_call_seen_running_is_watched_and_long_paths_are_not_kept() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    assert!(!denials.watching(t, tu, call(1)));
    let long = "/a".repeat(MAX_PATH_BYTES);
    denials.watch(t, tu, call(1), [long, "/b".to_owned()]);
    assert!(denials.watching(t, tu, call(1)));
    assert!(!denials.watching(t, turn(2), call(1)));
    assert_eq!(denials.unwatch(t, tu, call(1)), vec!["/b"]);
    assert!(!denials.watching(t, tu, call(1)));
}

#[test]
fn cards_dedupe_by_identity_else_by_folded_text() {
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let key = (thread(1), turn(1));
    let mut put_folder = |path: &str, card: Target| {
        let kind = folder(path);
        put(
            &mut denials,
            key,
            (&kind, true, true),
            (&card, &card),
            &mut minter,
        )
        .0
    };
    // `Other.` and `Other` are one folder by identity.
    let first = put_folder(r"D:\M\Other", target(r"D:\M\Other", Some(id(7))));
    let alias = put_folder(r"D:\M\Other.", target(r"D:\M\Other.", Some(id(7))));
    assert_eq!(
        (first, alias),
        (
            Recorded::Log {
                denial: denial(1),
                count: 1
            },
            Recorded::Log {
                denial: denial(1),
                count: 2
            }
        )
    );
    // With no identity, the folded text decides.
    let upper = Target::text("/Users/A/Deals", true);
    let lower = Target::text("/users/a/deals", true);
    let one = put_folder("/Users/A/Deals", upper);
    let two = put_folder("/users/a/deals", lower);
    assert!(matches!(one, Recorded::Log { count: 1, .. }), "{one:?}");
    assert!(matches!(two, Recorded::Log { count: 2, .. }), "{two:?}");
}

#[test]
fn a_card_not_offered_gives_way_to_an_offered_one_of_the_same_target() {
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let key = (thread(1), turn(1));
    let kind = folder("/home/a/Matters/Smith/x");
    let card = target("/home/a/Matters/Smith/x", Some(id(9)));
    let mut once = |offered: bool, os: bool| {
        put(
            &mut denials,
            key,
            (&kind, offered, os),
            (&card, &card),
            &mut minter,
        )
        .0
    };
    let claim = once(false, false);
    let confirmed = once(true, true);
    let repeat = once(false, false);
    assert_eq!(
        [claim, confirmed, repeat],
        [
            Recorded::Log {
                denial: denial(1),
                count: 1
            },
            Recorded::Log {
                denial: denial(2),
                count: 1
            },
            Recorded::Log {
                denial: denial(2),
                count: 2
            },
        ]
    );
    assert_eq!(denials.card_target(key.0, denial(2)), Some(&card));
    assert_eq!(denials.cards(key.0).len(), 1);
}

#[test]
fn one_refusal_reported_by_the_os_and_a_tool_call_counts_once() {
    let kind = folder("/srv/x");
    let asked = target("/srv/x", Some(id(3)));
    // The tool call first: it counts when recorded; the OS report's count takes it back.
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let (t, tu) = (thread(1), turn(1));
    let tool = put(
        &mut denials,
        (t, tu),
        (&kind, false, false),
        (&asked, &asked),
        &mut minter,
    );
    assert_eq!(tool.1, None);
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    let os = put(
        &mut denials,
        (t, tu),
        (&kind, true, true),
        (&asked, &asked),
        &mut minter,
    );
    assert_eq!(os.1, None);
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    let third = denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause;
    assert_eq!(third, Some((tu, 3)));

    // The OS first: the tool call's report of the same target adds nothing.
    let mut denials = Denials::default();
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    let _ = put(
        &mut denials,
        (t, tu),
        (&kind, true, true),
        (&asked, &asked),
        &mut minter,
    );
    let tool = put(
        &mut denials,
        (t, tu),
        (&kind, false, false),
        (&asked, &asked),
        &mut minter,
    );
    assert_eq!(tool.1, None);
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    let third = denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause;
    assert_eq!(third, Some((tu, 3)));

    // Tool-call reports of targets no OS report names count, each.
    let mut denials = Denials::default();
    for n in 0..2 {
        let other = target(&format!("/srv/{n}"), Some(id(100 + n)));
        let got = put(
            &mut denials,
            (t, tu),
            (&kind, false, false),
            (&other, &other),
            &mut minter,
        );
        assert_eq!(got.1, None);
    }
    let other = target("/srv/9", Some(id(109)));
    let got = put(
        &mut denials,
        (t, tu),
        (&kind, false, false),
        (&other, &other),
        &mut minter,
    );
    assert_eq!(got.1, Some((tu, 3)));
}

#[test]
fn only_an_os_report_of_the_same_file_by_identity_confirms() {
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let (t, tu) = (thread(1), turn(1));
    let kind = folder("/home/a/Matters/Smith/x");
    let file = target("/home/a/Matters/Smith/x", Some(id(5)));
    let missing = target("/home/a/Matters/Smith/x", None);
    // A tool call's own report confirms nothing.
    let _ = put(
        &mut denials,
        (t, tu),
        (&kind, false, false),
        (&file, &file),
        &mut minter,
    );
    assert!(!denials.confirmed(t, Some(tu), &file));
    let _ = put(
        &mut denials,
        (t, tu),
        (&kind, true, true),
        (&file, &file),
        &mut minter,
    );
    assert!(denials.confirmed(t, Some(tu), &file));
    // The same text with no identity is not the same file; nor is another turn.
    assert!(!denials.confirmed(t, Some(tu), &missing));
    assert!(!denials.confirmed(t, Some(turn(2)), &file));
}
