//! Subscriptions: a view's tail or `Reset` by `(epoch, cursor)`, the byte-capped patch window,
//! encode once, and event subscriptions with no gap, no duplicate and one `CaughtUp`.

use std::cell::{Cell, RefCell};

use clauseo_protocol::{Cursor, Seq, StopReason, StreamId, ViewEpoch};
use proptest::prelude::any;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

use crate::observe::{Events, View};
use crate::support::{Dir, FAKE, Running, Scenario, foreign_uuid, matter_with_thread};

/// A view subscription from no position (a `Reset`) and the turn that follows, collected until
/// the turn has ended and everything it published has arrived.
fn view_through_turn(dir: &Dir, host: &Running) -> (StreamId, View) {
    let (_, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let mut view = host.view(stream, None);
    view.until_items(1);
    assert!(view.starts_with_reset(), "{}", view.dump());
    let mut events = host.events(stream);
    let (turn, _) = host.prompt(thread, "Summarise the indemnity clause.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    host.settle();
    view.drain_ready();
    (stream, view)
}

/// A cursor value.
fn cursor(n: u64) -> Cursor {
    Cursor::new(n).unwrap()
}

/// The patches of `view` after `after`, as JSON.
fn patch_json_after(view: &View, after: u64) -> Vec<String> {
    view.patches()
        .filter(|p| p.cursor.get() > after)
        .map(|p| serde_json::to_string(p).unwrap())
        .collect()
}

#[test]
fn subscribe_tail_reset_and_new_epoch() {
    let dir = Dir::new(
        "subscribe_tail_reset_and_new_epoch",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (stream, first) = view_through_turn(&dir, &host);
    let epoch = first.batches[0].epoch;
    let start = first.patches().next().unwrap().cursor.get();
    let end = first.cursor().unwrap();
    assert!(end > start, "the turn published no patch: {}", first.dump());

    let mut tail = host.view(stream, Some((epoch, cursor(start))));
    tail.until("the tail", |v| v.cursor() == Some(end));
    assert_eq!(
        tail.resets(),
        0,
        "a current cursor got a Reset: {}",
        tail.dump()
    );
    assert_eq!(
        patch_json_after(&tail, start),
        patch_json_after(&first, start)
    );
    let cursors: Vec<u64> = tail.patches().map(|p| p.cursor.get()).collect();
    assert_eq!(cursors, (start + 1..=end).collect::<Vec<_>>());

    let stale = ViewEpoch::new(foreign_uuid(3)).unwrap();
    let mut other = host.view(stream, Some((stale, cursor(end))));
    other.until_items(1);
    assert!(
        other.starts_with_reset(),
        "a stale epoch got: {}",
        other.dump()
    );
    assert_eq!(other.batches[0].epoch, epoch);
    host.shutdown();

    let host = dir.start(1);
    let mut after_restart = host.view(stream, Some((epoch, cursor(end))));
    after_restart.until_items(1);
    assert!(
        after_restart.starts_with_reset(),
        "after a restart: {}",
        after_restart.dump()
    );
    assert_ne!(
        after_restart.batches[0].epoch, epoch,
        "a restart keeps no epoch"
    );
    host.shutdown();
}

#[test]
fn patch_window_capped_by_bytes() {
    const CAP: usize = 1024;
    let dir = Dir::new(
        "patch_window_capped_by_bytes",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start_with(0, |config| config.with_patch_window_bytes(CAP));
    let (stream, first) = view_through_turn(&dir, &host);
    let epoch = first.batches[0].epoch;
    let start = first.patches().next().unwrap().cursor.get();
    let end = first.cursor().unwrap();
    // Each live patch with its serialised length, as the window counts it.
    let sizes: Vec<(u64, usize)> = first
        .patches()
        .skip(1)
        .map(|p| (p.cursor.get(), serde_json::to_string(p).unwrap().len()))
        .collect();
    let suffix = |after: u64| -> usize {
        sizes
            .iter()
            .filter(|(c, _)| *c > after)
            .map(|(_, len)| len)
            .sum()
    };
    assert!(
        suffix(start) > CAP,
        "the turn's patches fit the cap: {sizes:?}"
    );

    let (mut resets, mut tails) = (0, 0);
    for after in start..end {
        let mut view = host.view(stream, Some((epoch, cursor(after))));
        view.until_items(1);
        if suffix(after) > CAP {
            assert!(
                view.starts_with_reset(),
                "cursor {after} fell out ({} bytes after it) but got: {}",
                suffix(after),
                view.dump()
            );
            resets += 1;
        } else {
            view.until("the tail", |v| v.cursor() == Some(end));
            assert_eq!(
                view.resets(),
                0,
                "cursor {after} is in the window: {}",
                view.dump()
            );
            let window: usize = view
                .patches()
                .map(|p| serde_json::to_string(p).unwrap().len())
                .sum();
            assert!(window <= CAP, "the tail after {after} holds {window} bytes");
            tails += 1;
        }
    }
    assert!(
        resets > 0 && tails > 0,
        "sizes {sizes:?} test only one side"
    );
    host.shutdown();
}

#[test]
fn view_events_encoded_once() {
    let dir = Dir::new(
        "view_events_encoded_once",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let mut one = host.view(stream, None);
    one.until_items(1);
    let mut two = host.view(stream, None);
    two.until_items(1);
    let mut events = host.events(stream);
    let (turn, _) = host.prompt(thread, "Summarise the indemnity clause.");
    events.until_turn_ended(turn);
    host.settle();
    one.drain_ready();
    two.drain_ready();

    assert_eq!(one.items.len(), two.items.len());
    assert!(one.items.len() > 1, "no live patches: {}", one.dump());
    // The first item of each is its own Reset; every live one is shared.
    for (a, b) in one.items.iter().zip(&two.items).skip(1) {
        assert!(a.ptr_eq(b), "two encodings of one batch: {}", a.json());
    }
    for item in one.items.iter().chain(&two.items) {
        let decoded = item.decode().unwrap();
        assert_eq!(item.json(), serde_json::to_string(&decoded).unwrap());
    }
    host.shutdown();
}

/// The seed of `subscribe_events_no_gap_no_dup`.
const SEED: [u8; 32] = *b"clauseo-host-subscribe-events-01";

/// One subscription opened by a proptest case: where it started and what it received.
struct Opened {
    after: u64,
    events: Events,
}

/// Checks `opened` holds exactly the seqs after its start through `head`, with one `CaughtUp`
/// between the replayed and the live envelopes.
fn check_subscription(opened: &Opened, head: u64) {
    let Opened { after, events } = opened;
    let seqs: Vec<u64> = events.seen.iter().map(|e| e.seq.get()).collect();
    assert_eq!(
        seqs,
        (after + 1..=head).collect::<Vec<_>>(),
        "after {after}"
    );
    assert_eq!(
        events.caught_up.len(),
        1,
        "after {after}: {}",
        events.dump()
    );
    let (at, last) = events.caught_up[0];
    let (replayed, live) = events.seen.split_at(at);
    assert!(replayed.iter().all(|e| e.seq <= last), "after {after}");
    assert!(live.iter().all(|e| e.seq > last), "after {after}");
}

/// 32 subscriptions from random seqs, opened while `files-multi-turn` runs its three turns:
/// cases 0, 8 and 16 each send the next prompt just before subscribing, so those replays race
/// the turn's commits. Each case checks its replay; at the end every subscription must hold
/// exactly `(after_seq, head]` with one `CaughtUp`.
#[test]
fn subscribe_events_no_gap_no_dup() {
    let dir = Dir::new(
        "subscribe_events_no_gap_no_dup",
        &[(FAKE, Scenario::Bundled("files-multi-turn"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("readme.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let main = RefCell::new(host.events(stream));
    let opened = RefCell::new(Vec::new());
    let turns = RefCell::new(Vec::new());
    let case = Cell::new(0_usize);
    let prompts = [
        "Write two notes.",
        "Revise and archive.",
        "Delete the note.",
    ];

    let config = Config {
        cases: 32,
        failure_persistence: None,
        max_shrink_iters: 0,
        ..Config::default()
    };
    let mut runner =
        TestRunner::new_with_rng(config, TestRng::from_seed(RngAlgorithm::ChaCha, &SEED));
    let result = runner.run(&any::<u8>(), |fraction| {
        let n = case.replace(case.get() + 1);
        if n.is_multiple_of(8)
            && let Some(prompt) = prompts.get(n / 8)
        {
            turns.borrow_mut().push(host.prompt(thread, prompt).0);
        }
        let known = main.borrow_mut().drain_ready().head();
        let after = known * u64::from(fraction) / 255;
        let mut events = host.events_after(stream, (after > 0).then(|| Seq::new(after).unwrap()));
        events.until_caught_up();
        let (at, last) = events.caught_up[0];
        let seqs: Vec<u64> = events.seen[..at].iter().map(|e| e.seq.get()).collect();
        proptest::prop_assert_eq!(seqs, (after + 1..=last.get()).collect::<Vec<_>>());
        opened.borrow_mut().push(Opened { after, events });
        Ok(())
    });
    if let Err(error) = result {
        panic!("{error}");
    }

    let mut main = main.into_inner();
    for turn in turns.into_inner() {
        assert_eq!(main.until_turn_ended(turn).stop, StopReason::EndTurn);
    }
    host.settle();
    main.drain_ready().assert_no_mismatch();
    let head = main.head();
    let mut opened = opened.into_inner();
    assert_eq!(opened.len(), 32);
    for one in &mut opened {
        one.events.drain_ready();
        check_subscription(one, head);
    }
    host.shutdown();
}
