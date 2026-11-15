//! Sending crash reports: when (1 to 24 h after the crash, never within an hour of a counts
//! POST), what (the stored bytes), and what each answer does.

use clauseo_counts::schema::{RendererGoneReason, Time};
use clauseo_counts::{Counts, Crashes, Switch, UploadError, UploadPath, send_crash_due};

use crate::crash::{LOCKED, ON, crash_files, panic_at, recording};
use crate::support::{FakeUpload, HOUR, Scratch, at, fill_days, gunzip, send};

/// Desktop's tick at `now`, crash half, with `switch`.
fn crash_tick(crashes: &Crashes, counts: &Counts, upload: &FakeUpload, now: Time, switch: Switch) {
    futures::executor::block_on(send_crash_due(crashes, counts, upload, now, &|| switch));
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
            crashes
                .record_renderer_gone(RendererGoneReason::Crashed, at(3, 23 * HOUR + n))
                .unwrap();
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
        if reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .is_multiple_of(2)
        {
            ON
        } else {
            LOCKED
        }
    };
    let mut now = at(2, 120);
    while now <= at(3, 2 * HOUR) {
        futures::executor::block_on(send_crash_due(&crashes, &counts, &upload, now, &switch));
        now = now.plus(60);
    }
    assert!(upload.seen().is_empty());
    assert_eq!(crashes.queued(), Vec::<String>::new());
}
