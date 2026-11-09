//! The intake's bookkeeping on a fake feed: the rate cap, the pause (in a row, in the turn, a
//! `Dropped`'s count, once), "in a row" starting again, and one card per kind and target.

use std::time::Duration;

use clauseo_protocol::{DenialId, DenialKind, ThreadId, ToolCallId, TurnId};
use uuid::Uuid;

use super::{
    Admitted, DENIALS_PER_WINDOW, Denials, MAX_CARDS_PER_TURN, PAUSE_IN_TURN, Rate, Record,
    Recorded,
};
use crate::actor::turn::denials::offer::identity::Target;
use crate::error::Error;

fn v7(n: u64) -> Uuid {
    let timestamp: u128 = 0x0190_0000_0000;
    Uuid::from_u128((timestamp << 80) | (0x7 << 76) | (0b10 << 62) | u128::from(n))
}

fn thread(n: u64) -> ThreadId {
    ThreadId::new(v7(n)).unwrap()
}

fn turn(n: u64) -> TurnId {
    TurnId::new(v7(100 + n)).unwrap()
}

fn denial(n: u64) -> DenialId {
    DenialId::new(v7(1000 + n)).unwrap()
}

fn call(n: u64) -> ToolCallId {
    ToolCallId::new(v7(2000 + n)).unwrap()
}

fn folder(path: &str) -> DenialKind {
    DenialKind::Folder {
        path: path.into(),
        other_matter: None,
    }
}

const MS: Duration = Duration::from_millis(1);

/// Mints denial ids 1, 2, ... in order.
struct Minter(u64);

impl Minter {
    fn mint(&mut self) -> impl FnOnce() -> Result<DenialId, Error> + '_ {
        move || {
            self.0 += 1;
            Ok(denial(self.0))
        }
    }
}

/// Records an offered card of `kind` from the OS, its target its text.
fn put(
    denials: &mut Denials,
    t: ThreadId,
    (turn, running): (Option<TurnId>, Option<TurnId>),
    kind: &DenialKind,
    minter: &mut Minter,
) -> Recorded {
    let text = match kind {
        DenialKind::Folder { path, .. } | DenialKind::File { path, .. } => path.clone(),
        DenialKind::Site { .. } | DenialKind::UpstreamAuth { .. } | DenialKind::Unknown { .. } => {
            String::new()
        }
    };
    let card = Target::text(&text, false);
    let record = Record {
        kind,
        offered: true,
        card: &card,
        asked: Some(&card),
        os: true,
    };
    denials
        .record(t, turn, running, record, minter.mint())
        .unwrap()
        .0
}

#[test]
fn at_most_twenty_a_second_pass_the_intake() {
    let mut denials = Denials::default();
    let (t, now) = (thread(1), Duration::from_secs(10));
    // Not counted toward the pause, so the cap alone decides.
    let passed = (0..30)
        .filter(|_| denials.admit(t, Some(turn(1)), now, 0, Rate::Counted).pass)
        .count();
    assert_eq!(passed, DENIALS_PER_WINDOW as usize);
    // A second later the window starts again.
    let later = now + Duration::from_secs(1);
    assert!(
        denials
            .admit(t, Some(turn(1)), later, 0, Rate::Counted)
            .pass
    );
    // Another thread has its own window.
    assert!(denials.admit(thread(2), None, now, 0, Rate::Counted).pass);
}

#[test]
fn three_counted_in_a_row_pause_once() {
    let mut denials = Denials::default();
    let t = thread(1);
    let admit = |d: &mut Denials, counted| d.admit(t, Some(turn(1)), MS, counted, Rate::Counted);
    assert_eq!(admit(&mut denials, 1).pause, None);
    // A read-only refusal inside the matter is not counted.
    assert_eq!(admit(&mut denials, 0).pause, None);
    assert_eq!(admit(&mut denials, 1).pause, None);
    assert_eq!(
        admit(&mut denials, 1),
        Admitted {
            pass: true,
            pause: Some((turn(1), 3))
        }
    );
    // Once per turn.
    assert_eq!(admit(&mut denials, 1).pause, None);
    // The next turn counts afresh.
    let next = denials.admit(t, Some(turn(2)), MS, 1, Rate::Counted);
    assert_eq!(next.pause, None);
}

#[test]
fn no_turn_never_pauses() {
    let mut denials = Denials::default();
    for _ in 0..5 {
        assert_eq!(
            denials.admit(thread(1), None, MS, 1, Rate::Counted).pause,
            None
        );
    }
}

#[test]
fn a_clean_completed_tool_call_starts_in_a_row_again() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    denials.admit(t, Some(tu), MS, 1, Rate::Counted);
    denials.admit(t, Some(tu), MS, 1, Rate::Counted);
    // The call that saw the denials ends: it may have been theirs, so the run goes on.
    denials.tool_ended(t, tu, true);
    // A failed call closes the span without starting again.
    denials.tool_ended(t, tu, false);
    // A call completed with no denial since the last end: in a row starts again.
    denials.tool_ended(t, tu, true);
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    assert_eq!(denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause, None);
    // 5 in the turn, 3 in a row now.
    assert_eq!(
        denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause,
        Some((tu, 5))
    );
}

#[test]
fn twenty_in_a_turn_pause_even_when_not_in_a_row() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    let mut paused = None;
    for n in 1..=PAUSE_IN_TURN {
        let admitted = denials.admit(t, Some(tu), MS * n, 1, Rate::Counted);
        if admitted.pause.is_some() {
            paused = Some((n, admitted.pause));
            break;
        }
        if n % 2 == 0 {
            // Two denials, a call ends (theirs), a clean call ends: never three in a row.
            denials.tool_ended(t, tu, true);
            denials.tool_ended(t, tu, true);
        }
    }
    assert_eq!(paused, Some((PAUSE_IN_TURN, Some((tu, 20)))));
}

#[test]
fn a_dropped_count_adds_to_the_pause_and_is_not_rated() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    let admitted = denials.admit(t, Some(tu), MS, 25, Rate::Unrated);
    assert_eq!(
        admitted,
        Admitted {
            pass: false,
            pause: Some((tu, 25))
        }
    );
}

#[test]
fn denials_past_the_cap_still_count_toward_the_pause() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    for _ in 0..DENIALS_PER_WINDOW {
        denials.admit(t, Some(tu), MS, 0, Rate::Counted);
    }
    let over = denials.admit(t, Some(tu), MS, 1, Rate::Counted);
    assert!(!over.pass);
    denials.admit(t, Some(tu), MS, 1, Rate::Counted);
    assert_eq!(
        denials.admit(t, Some(tu), MS, 1, Rate::Counted).pause,
        Some((tu, 3))
    );
}

#[test]
fn a_repeat_grows_its_count_a_new_turn_gives_a_new_card_a_late_one_joins_its_turn() {
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let (t, tu) = (thread(1), turn(1));
    let deals = folder("/home/a/Deals");
    let first = put(&mut denials, t, (Some(tu), Some(tu)), &deals, &mut minter);
    assert_eq!(
        first,
        Recorded::Log {
            denial: denial(1),
            count: 1
        }
    );
    let again = put(&mut denials, t, (Some(tu), Some(tu)), &deals, &mut minter);
    assert_eq!(
        again,
        Recorded::Log {
            denial: denial(1),
            count: 2
        }
    );
    let other = put(
        &mut denials,
        t,
        (Some(tu), Some(tu)),
        &folder("/srv"),
        &mut minter,
    );
    assert_eq!(
        other,
        Recorded::Log {
            denial: denial(2),
            count: 1
        }
    );
    let next = put(
        &mut denials,
        t,
        (Some(turn(2)), Some(turn(2))),
        &deals,
        &mut minter,
    );
    assert_eq!(
        next,
        Recorded::Log {
            denial: denial(3),
            count: 1
        }
    );
    // A late check for the ended turn goes to that turn's card, deduped there.
    let late = put(
        &mut denials,
        t,
        (Some(tu), Some(turn(2))),
        &deals,
        &mut minter,
    );
    assert_eq!(
        late,
        Recorded::Log {
            denial: denial(1),
            count: 3
        }
    );
    let kept = put(
        &mut denials,
        t,
        (Some(turn(2)), Some(turn(2))),
        &deals,
        &mut minter,
    );
    assert_eq!(
        kept,
        Recorded::Log {
            denial: denial(3),
            count: 2
        }
    );
    // One older than the last ended turn is not recorded.
    let _ = denials.admit(t, Some(turn(3)), MS, 0, Rate::Unrated);
    let stale = put(
        &mut denials,
        t,
        (Some(tu), Some(turn(3))),
        &deals,
        &mut minter,
    );
    assert_eq!(stale, Recorded::Stale);
}

#[test]
fn a_turn_records_at_most_its_cap_of_cards() {
    let mut denials = Denials::default();
    let mut minter = Minter(0);
    let (t, tu) = (thread(1), turn(1));
    for n in 0..MAX_CARDS_PER_TURN {
        let kind = folder(&format!("/srv/{n}"));
        let got = put(&mut denials, t, (Some(tu), Some(tu)), &kind, &mut minter);
        assert!(matches!(got, Recorded::Log { count: 1, .. }));
    }
    let over =
        |d: &mut Denials, m: &mut Minter| put(d, t, (Some(tu), Some(tu)), &folder("/srv/over"), m);
    assert_eq!(
        over(&mut denials, &mut minter),
        Recorded::Capped { first: true }
    );
    assert_eq!(
        over(&mut denials, &mut minter),
        Recorded::Capped { first: false }
    );
    // A repeat of a kept card still counts.
    let repeat = put(
        &mut denials,
        t,
        (Some(tu), Some(tu)),
        &folder("/srv/0"),
        &mut minter,
    );
    assert!(matches!(repeat, Recorded::Log { count: 2, .. }));
}

#[test]
fn watched_calls_keep_their_paths_until_unwatched() {
    let mut denials = Denials::default();
    let (t, tu) = (thread(1), turn(1));
    denials.watch(t, tu, call(1), ["/a".to_owned(), "/a".to_owned()]);
    denials.watch(t, tu, call(1), ["/b".to_owned()]);
    assert_eq!(denials.unwatch(t, tu, call(1)), vec!["/a", "/b"]);
    assert_eq!(denials.unwatch(t, tu, call(1)), Vec::<String>::new());
    // A new turn forgets the last turn's calls.
    denials.watch(t, tu, call(2), ["/c".to_owned()]);
    assert_eq!(denials.unwatch(t, turn(2), call(2)), Vec::<String>::new());
}

mod intake;
