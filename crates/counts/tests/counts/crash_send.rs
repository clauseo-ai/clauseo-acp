//! Sending crash reports: when (1 to 24 h after the crash, never within an hour of a counts
//! POST), what (the stored bytes), and what each answer does.

use std::future::ready;
use std::pin::pin;

use clauseo_counts::schema::{RendererGoneReason, Time};
use clauseo_counts::{Counts, Crashes, Switch, UploadError, UploadPath, send_crash_due};
use futures::executor::block_on;
use futures::future::poll_immediate;

use crate::crash::{LOCKED, ON, crash_files, panic_at, recording, renderer_gone_in_a_new_run};
use crate::support::{FakeUpload, HOUR, Scratch, at, fill_days, gunzip, plant, send};

/// Desktop's tick at `now`, crash half, with `switch`.
fn crash_tick(crashes: &Crashes, counts: &Counts, upload: &FakeUpload, now: Time, switch: Switch) {
    block_on(send_crash_due(crashes, counts, upload, now, || {
        ready(Some(switch))
    }));
}

/// Ticks every minute from `from` to `to`; the times of the crash POSTs.
fn crash_run(
    crashes: &Crashes,
    counts: &Counts,
    upload: &FakeUpload,
    from: Time,
    to: Time,
) -> Vec<Time> {
    let mut posts = Vec::new();
    let mut now = from;
    while now <= to {
        let before = upload.seen().len();
        crash_tick(crashes, counts, upload, now, ON);
        if upload.seen().len() > before {
            posts.push(now);
        }
        now = now.plus(60);
    }
    posts
}

#[test]
fn a_report_goes_once_1_to_24_hours_after_the_crash_as_stored() {
    for seed in 0..20 {
        let scratch = Scratch::new(&format!("crash-send-{seed}"));
        let (counts, _) = recording(&scratch);
        let crashes = Crashes::open(scratch.path(), crate::support::about(), seed);
        crashes.ingest(&counts, ON, at(2, 0)).unwrap();
        let crash = at(2, 2 * HOUR);
        panic_at(&crashes, crash);
        crashes.ingest(&counts, ON, crash.plus(60)).unwrap();
        let name = crashes.queued()[0].clone();
        let stored = std::fs::read(scratch.path().join("crash/queue").join(&name)).unwrap();

        let upload = FakeUpload::answering(&[Ok(204)]);
        let posts = crash_run(
            &crashes,
            &counts,
            &upload,
            crash.plus(60),
            crash.plus(26 * HOUR),
        );
        assert_eq!(posts.len(), 1, "seed {seed}");
        let after = posts[0].since(crash);
        assert!(
            (HOUR..=24 * HOUR + 60).contains(&after),
            "seed {seed}: {after}"
        );
        let seen = upload.seen();
        assert_eq!(seen[0].path, UploadPath::Crash);
        assert_eq!(
            gunzip(&seen[0].body),
            stored,
            "the bytes sent are the bytes stored"
        );
        assert_eq!(crashes.queued(), Vec::<String>::new());
    }
}

#[test]
fn a_report_waiting_since_before_a_restart_goes_within_the_hour() {
    let scratch = Scratch::new("crash-send-late");
    let (counts, crashes) = recording(&scratch);
    panic_at(&crashes, at(2, 0));
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    // The app was closed for two days.
    let next = Crashes::open(scratch.path(), crate::support::about(), 3);
    let upload = FakeUpload::answering(&[Ok(204)]);
    let posts = crash_run(&next, &counts, &upload, at(4, 0), at(4, 2 * HOUR));
    assert_eq!(posts.len(), 1);
    assert!(posts[0].since(at(4, 0)) <= HOUR + 60);
}

#[test]
fn a_400_deletes_and_a_429_503_or_no_answer_keeps_for_later() {
    let scratch = Scratch::new("crash-send-answers");
    let (counts, crashes) = recording(&scratch);
    crashes
        .record_renderer_gone(RendererGoneReason::Crashed, at(2, 0))
        .unwrap();
    let upload = FakeUpload::answering(&[Err(UploadError), Ok(429), Ok(503), Ok(400)]);
    let posts = crash_run(&crashes, &counts, &upload, at(2, 60), at(5, 0));
    assert_eq!(posts.len(), 4, "{posts:?}");
    for pair in posts.windows(2) {
        let gap = pair[1].since(pair[0]);
        assert!((HOUR..=6 * HOUR + 60).contains(&gap), "{gap}");
    }
    assert_eq!(crashes.queued(), Vec::<String>::new());
    assert_eq!(crash_files(&scratch), Vec::<String>::new());
}

#[test]
fn a_report_never_goes_within_an_hour_of_a_counts_post_either_way() {
    for seed in 0..8 {
        let scratch = Scratch::new(&format!("crash-send-gap-{seed}"));
        let (counts, _) = recording(&scratch);
        let crashes = Crashes::open(scratch.path(), crate::support::about(), seed);
        crashes.ingest(&counts, ON, at(2, 0)).unwrap();
        fill_days(&counts, 2, 3);
        for n in 0..3 {
            renderer_gone_in_a_new_run(&scratch, at(3, 23 * HOUR + n));
        }
        // A failed counts POST first, so its retries spread over the day.
        let upload = FakeUpload::answering(&[Err(UploadError), Err(UploadError), Ok(200)]);
        // Both halves of desktop's tick, every minute for two days.
        let mut posts: Vec<(Time, UploadPath)> = Vec::new();
        let mut now = at(4, 0);
        while now <= at(6, 0) {
            let before = upload.seen().len();
            counts.tick(now).unwrap();
            send(&counts, &upload, now);
            crash_tick(&crashes, &counts, &upload, now, ON);
            posts.extend(upload.seen()[before..].iter().map(|seen| (now, seen.path)));
            now = now.plus(60);
        }
        let crash: Vec<Time> = posts
            .iter()
            .filter(|(_, path)| *path == UploadPath::Crash)
            .map(|(at, _)| *at)
            .collect();
        let other: Vec<Time> = posts
            .iter()
            .filter(|(_, path)| *path != UploadPath::Crash)
            .map(|(at, _)| *at)
            .collect();
        assert_eq!(crash.len(), 3, "seed {seed}: {posts:?}");
        assert!(!other.is_empty(), "seed {seed}");
        for c in &crash {
            for o in &other {
                let gap = c.max(o).since(*c.min(o));
                assert!(gap >= HOUR, "seed {seed}: {c:?} and {o:?}");
            }
        }
    }
}

#[test]
fn a_lock_read_just_before_the_post_sends_nothing() {
    let scratch = Scratch::new("crash-send-lock");
    let (counts, crashes) = recording(&scratch);
    crashes
        .record_renderer_gone(RendererGoneReason::Crashed, at(2, 0))
        .unwrap();
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    let upload = FakeUpload::answering(&[Ok(204)]);
    // ON for the ingest, LOCKED when the POST is about to go.
    let reads = std::sync::atomic::AtomicUsize::new(0);
    let switch = || {
        let first = reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .is_multiple_of(2);
        ready(Some(if first { ON } else { LOCKED }))
    };
    let mut now = at(2, 120);
    while now <= at(3, 2 * HOUR) {
        block_on(send_crash_due(&crashes, &counts, &upload, now, switch));
        now = now.plus(60);
    }
    assert!(upload.seen().is_empty());
    assert_eq!(crashes.queued(), Vec::<String>::new());
}

#[test]
fn no_answer_about_the_switch_changes_and_sends_nothing() {
    let scratch = Scratch::new("crash-send-no-answer");
    let (counts, crashes) = recording(&scratch);
    panic_at(&crashes, at(2, 0));
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    let upload = FakeUpload::answering(&[Ok(204)]);
    let mut now = at(2, 120);
    while now <= at(3, 2 * HOUR) {
        block_on(send_crash_due(&crashes, &counts, &upload, now, || {
            ready(None::<Switch>)
        }));
        now = now.plus(60);
    }
    assert!(upload.seen().is_empty());
    assert_eq!(crashes.queued().len(), 1);
}

/// Over the Worker's hourly cap `/v1/crash` answers `429` (a `503` when it is down): the
/// report stays on disk as it was, goes again at its next try, and the `204` deletes it.
#[test]
fn a_429_or_503_keeps_the_report_and_the_next_try_sends_it() {
    for refusal in [429, 503] {
        let scratch = Scratch::new(&format!("crash-send-{refusal}"));
        let (counts, crashes) = recording(&scratch);
        panic_at(&crashes, at(2, 0));
        crashes.ingest(&counts, ON, at(2, 60)).unwrap();
        let name = crashes.queued()[0].clone();
        let path = scratch.path().join("crash/queue").join(&name);
        let stored = std::fs::read(&path).unwrap();
        let upload = FakeUpload::answering(&[Ok(refusal), Ok(204)]);

        // Tick until the first POST, which the Worker refuses.
        let mut first = at(2, 120);
        loop {
            crash_tick(&crashes, &counts, &upload, first, ON);
            if !upload.seen().is_empty() || first > at(3, 2 * HOUR) {
                break;
            }
            first = first.plus(60);
        }
        let now = first.plus(60);
        assert_eq!(upload.seen().len(), 1, "{refusal}: refused once");
        assert_eq!(crashes.queued(), vec![name.clone()], "{refusal}: kept");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            stored,
            "{refusal}: unchanged"
        );

        let second = crash_run(&crashes, &counts, &upload, now, first.plus(7 * HOUR));
        assert_eq!(second.len(), 1, "{refusal}: {second:?}");
        assert!(second[0].since(first) >= HOUR, "{refusal}");
        let seen = upload.seen();
        assert_eq!(
            gunzip(&seen[1].body),
            stored,
            "{refusal}: the same report again"
        );
        assert_eq!(
            crashes.queued(),
            Vec::<String>::new(),
            "{refusal}: sent, so deleted"
        );
        assert_eq!(crash_files(&scratch), Vec::<String>::new());
    }
}

/// A queued file changed on disk is read through the schema before it goes: an extra field
/// fails it, and the file is deleted, never sent.
#[test]
fn a_tampered_report_is_deleted_not_sent() {
    let scratch = Scratch::new("crash-send-tampered");
    let (counts, crashes) = recording(&scratch);
    panic_at(&crashes, at(2, 0));
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    let name = crashes.queued()[0].clone();
    let path = scratch.path().join("crash/queue").join(&name);
    let stored = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    let tampered = stored.replacen('{', r#"{"message":"Mehra v. Shah","#, 1);
    plant(&path, tampered.as_bytes());

    let upload = FakeUpload::answering(&[Ok(204)]);
    let posts = crash_run(&crashes, &counts, &upload, at(2, 120), at(3, 2 * HOUR));
    assert_eq!(posts, Vec::<Time>::new());
    assert!(upload.seen().is_empty());
    assert_eq!(crash_files(&scratch), Vec::<String>::new());
}

/// Counts turned off while a crash report's POST is in flight cancel it: the request is dropped,
/// its answer never read, and every report is deleted at once.
#[test]
fn counts_off_cancels_a_crash_post_in_flight_and_empties_the_queue() {
    let scratch = Scratch::new("crash-send-cancel");
    let (counts, crashes) = recording(&scratch);
    panic_at(&crashes, at(2, 0));
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    let upload = FakeUpload::hanging();
    let mut now = at(2, 120);
    loop {
        assert!(now <= at(3, 2 * HOUR), "the report never went");
        let mut tick = pin!(send_crash_due(&crashes, &counts, &upload, now, || {
            ready(Some(ON))
        }));
        if block_on(poll_immediate(&mut tick)).is_none() {
            // In flight: the lawyer turns counts off.
            assert_eq!(upload.seen().len(), 1);
            counts.set_enabled(false).unwrap();
            assert_eq!(
                block_on(poll_immediate(&mut tick)),
                Some(()),
                "cancelled at once"
            );
            break;
        }
        now = now.plus(60);
    }
    assert_eq!(crashes.queued(), Vec::<String>::new());
    assert!(!scratch.path().join("crash").exists());
}
