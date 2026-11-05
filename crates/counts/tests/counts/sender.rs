//! The sender, on a manual clock and a fake `Upload`.

use clauseo_counts::schema::Time;
use clauseo_counts::schema::{
    Agent, AnswerEnded, AnswerOutcome, Crash, CrashKind, CrashProcess, Event, MAX_COUNTS_GZIP,
    MAX_COUNTS_JSON, Took, parse_batch, parse_tally,
};
use clauseo_counts::{Counts, Outcome, UploadError, UploadPath};

use crate::support::*;

/// Days 2 to `to` queued, and counts reopened at `start` (a new run of the app).
fn queued_then_started(scratch: &Scratch, to: i64) -> Counts {
    let counts = saving(scratch);
    fill_days(&counts, 2, to);
    drop(counts);
    saving(scratch)
}

/// The day sends among `sent`.
fn day_sends(sent: Vec<(Time, Seen)>) -> Vec<(Time, Seen)> {
    sent.into_iter()
        .filter(|(_, seen)| seen.path == UploadPath::Counts)
        .collect()
}

#[test]
fn one_post_a_day_0_to_10_minutes_after_start_and_after_midnight() {
    let scratch = Scratch::new("daily");
    let counts = queued_then_started(&scratch, 3);
    let upload = FakeUpload::answering(&[Ok(200)]);
    let start = at(4, 9 * HOUR);
    let sent = day_sends(run(&counts, &upload, start, at(4, DAY - 1), 10));
    assert_eq!(sent.len(), 1, "one POST on day 4: {sent:?}");
    let (when, seen) = &sent[0];
    assert!(
        when.since(start) <= 600,
        "{} s after start",
        when.since(start)
    );
    let batch = batch_of(seen);
    assert_eq!(
        batch.days.iter().map(|d| d.day).collect::<Vec<_>>(),
        vec![day(2), day(3)]
    );

    // Day 4 counted something, so day 5 has it to send: 0 to 10 minutes after midnight, once.
    counts.bump(COLD);
    let sent = day_sends(run(
        &counts,
        &upload,
        at(4, DAY - 1).plus(1),
        at(5, DAY - 1),
        10,
    ));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].0.since(at(5, 0)) <= 600);
    assert_eq!(batch_of(&sent[0].1).days[0].day, day(4));
    assert_eq!(counts.queued_days().len(), 0);
}

#[test]
fn a_send_holds_at_most_7_days_gzip_under_16_kb_oldest_first() {
    let scratch = Scratch::new("batch");
    let counts = queued_then_started(&scratch, 11);
    assert_eq!(counts.queued_days().len(), 10);
    let upload = FakeUpload::answering(&[Ok(200)]);
    let sent = day_sends(run(&counts, &upload, at(12, 0), at(12, DAY - 1), 60));
    assert_eq!(sent.len(), 1);
    let seen = &sent[0].1;
    assert!(seen.body.len() <= MAX_COUNTS_GZIP);
    let json = gunzip(&seen.body);
    let batch = parse_batch(&json, day(12)).unwrap();
    assert_eq!(
        batch.days.iter().map(|d| d.day).collect::<Vec<_>>(),
        (2..=8).map(day).collect::<Vec<_>>()
    );
    assert_eq!(counts.queued_days(), (9..=11).map(day).collect::<Vec<_>>());
}

/// Bumps `n` distinct `answer_ended` cells.
fn many_cells(counts: &Counts, n: usize) {
    let mut made = 0;
    for agent in Agent::ALL {
        for outcome in AnswerOutcome::ALL {
            for took in Took::ALL {
                for first in std::iter::once(None).chain(Took::ALL.iter().copied().map(Some)) {
                    if made == n {
                        return;
                    }
                    counts.bump(Event::AnswerEnded(AnswerEnded {
                        agent: *agent,
                        outcome: *outcome,
                        took: *took,
                        first_words: first,
                    }));
                    made += 1;
                }
            }
        }
    }
}

#[test]
fn a_body_too_large_is_cut_to_fewer_days_and_a_day_too_large_alone_is_dropped() {
    let scratch = Scratch::new("cut");
    let counts = saving(&scratch);
    for d in 2..=8 {
        counts.tick(at(d, HOUR)).unwrap();
        // About 25 KB of JSON a day: 7 of them pass the 64 KB cap.
        many_cells(&counts, if d == 2 { 1540 } else { 300 });
    }
    counts.tick(at(9, 0)).unwrap();
    let upload = FakeUpload::answering(&[Ok(200)]);
    let sent = day_sends(run(&counts, &upload, at(9, 0), at(9, 3600), 60));
    assert_eq!(sent.len(), 1);
    let json = gunzip(&sent[0].1.body);
    assert!(json.len() <= MAX_COUNTS_JSON && sent[0].1.body.len() <= MAX_COUNTS_GZIP);
    let batch = parse_batch(&json, day(9)).unwrap();
    let days: Vec<_> = batch.days.iter().map(|d| d.day).collect();
    // Day 2 (every combination, over 64 KB alone) is dropped; the rest go oldest first.
    assert_eq!(days[0], day(3));
    assert!(days.len() < 6, "{days:?}");
    assert!(!counts.queued_days().contains(&day(2)));
}

#[test]
fn retries_come_at_1_2_4_then_every_4_hours_until_24_hours() {
    let scratch = Scratch::new("retries");
    let counts = queued_then_started(&scratch, 2);
    let upload = FakeUpload::answering(&[Ok(503)]);
    let sent = run(&counts, &upload, at(3, 0), at(4, DAY - 1), 60);
    let first = sent[0].0;
    let hours: Vec<u64> = sent
        .iter()
        .map(|(when, _)| when.since(first) / 60)
        .collect();
    let mut want: Vec<u64> = [0, 1, 3, 7, 11, 15, 19, 23]
        .iter()
        .map(|h| h * 60)
        .collect();
    // Then the next daily send, 0 to 10 minutes after the next midnight, starts a new round.
    let next = sent[8].0;
    // The first step of the 60 s clock at or after the send time.
    assert!(next.since(at(4, 0)) < 660, "{hours:?}");
    want.push(next.since(first) / 60);
    assert_eq!(&hours[..9], &want[..]);
    // The days stay queued through all of it.
    assert_eq!(counts.queued_days(), vec![day(2)]);
}

#[test]
fn no_answer_counts_as_a_failed_try_and_a_2xx_ends_the_round() {
    let scratch = Scratch::new("network");
    let counts = queued_then_started(&scratch, 2);
    let upload = FakeUpload::answering(&[Err(UploadError), Ok(429), Ok(204)]);
    let sent = run(&counts, &upload, at(3, 0), at(3, DAY - 1), 60);
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2].0.since(sent[0].0), 3 * HOUR);
    assert_eq!(counts.queued_days().len(), 0);
}

#[test]
fn a_400_drops_those_days() {
    let scratch = Scratch::new("bad-request");
    let counts = queued_then_started(&scratch, 3);
    let upload = FakeUpload::answering(&[Ok(400)]);
    let sent = run(&counts, &upload, at(4, 0), at(4, DAY - 1), 60);
    assert_eq!(sent.len(), 1);
    assert_eq!(counts.queued_days().len(), 0);
    assert_eq!(files(&counts_dir(&scratch).join("queue")).len(), 0);
}

#[test]
fn one_post_in_flight_and_one_with_no_answer_for_10_minutes_fails() {
    let scratch = Scratch::new("in-flight");
    let counts = queued_then_started(&scratch, 2);
    let start = at(3, 0);
    counts.tick(start).unwrap();
    let (offset, post) = (0..=600)
        .find_map(|s| counts.due(start.plus(s)).pop().map(|p| (s, p)))
        .unwrap();
    let went = start.plus(offset);
    assert_eq!(counts.due(went.plus(1)).len(), 0);
    assert_eq!(counts.due(went.plus(599)).len(), 0);
    // No answer at 10 minutes: a failed try, retried an hour after it went.
    assert_eq!(counts.due(went.plus(600)).len(), 0);
    // A late answer to it changes nothing.
    counts.sent(post.id(), Outcome::Status(200));
    assert_eq!(counts.queued_days(), vec![day(2)]);
    let retry = (601..=2 * HOUR).find(|&s| !counts.due(went.plus(s)).is_empty());
    assert_eq!(retry, Some(HOUR));
}

#[test]
fn the_tally_goes_alone_at_a_random_hour_in_the_week_after_its_week() {
    let scratch = Scratch::new("tally");
    let counts = saving(&scratch);
    counts.tick(at(3, 0)).unwrap();
    counts.bump(COLD);
    counts.bump(Event::Crash(Crash {
        process: CrashProcess::Webview,
        kind: CrashKind::RendererGone,
        after_launch: false,
    }));
    let upload = FakeUpload::answering(&[Ok(200)]);
    // Week 45 is Nov 2 to 8; week 46 Nov 9 to 15.
    let sent = run(&counts, &upload, at(3, 60), at(16, 0), 300);
    let tallies: Vec<_> = sent
        .iter()
        .filter(|(_, seen)| seen.path == UploadPath::Tally)
        .collect();
    assert_eq!(tallies.len(), 1, "{sent:?}");
    let (when, seen) = tallies[0];
    assert!(*when >= at(9, 0) && *when < at(16, 0));
    let tally = parse_tally(&gunzip(&seen.body), day(9).week()).unwrap();
    assert_eq!(tally.week.to_string(), "2026-W45");
    assert_eq!(tally.cells.crash.len(), 1);
    assert_eq!(tally.cells.active_week.len(), 1);
    // Alone: the day sends are other POSTs, none within the hour.
    for (other, seen) in &sent {
        if seen.path == UploadPath::Counts {
            assert!(when.since(*other).max(other.since(*when)) >= HOUR);
        }
    }
}
