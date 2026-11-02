//! Policy (coordinator decision 9): off from the start writes nothing; off later deletes the
//! folder at the next open or reload; the sender reads the effective value before every POST.
//! Also: a file that does not read as its type is removed, never sent.

use std::sync::atomic::{AtomicUsize, Ordering};

use clauseo_counts::{Counts, Mode, send_due};

use crate::support::*;

#[test]
fn policy_off_from_the_start_writes_nothing() {
    let scratch = Scratch::new("policy-start");
    let counts = Counts::open(scratch.path(), about(), false, false, SEED);
    assert_eq!(counts.mode(), Mode::Off);
    fill_days(&counts, 2, 3);
    counts.first_act(true).unwrap();
    let upload = FakeUpload::answering(&[Ok(200)]);
    run(&counts, &upload, at(4, 0), at(5, 0), 60);
    assert_eq!(upload.seen().len(), 0);
    assert!(!counts_dir(&scratch).exists());
    assert_eq!(files(scratch.path()), ["first_act_done"]);
}

#[test]
fn policy_after_the_queue_exists_deletes_it_at_the_next_open_and_sends_nothing() {
    let scratch = Scratch::new("policy-open");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    assert_ne!(files(&counts_dir(&scratch)).len(), 0);
    drop(counts);
    let counts = Counts::open(scratch.path(), about(), false, true, SEED);
    assert!(!counts_dir(&scratch).exists());
    let upload = FakeUpload::answering(&[Ok(200)]);
    run(&counts, &upload, at(4, 0), at(5, 0), 60);
    assert_eq!(upload.seen().len(), 0);
}

#[test]
fn policy_at_a_settings_reload_deletes_the_folder() {
    let scratch = Scratch::new("policy-reload");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    counts.set_enabled(false).unwrap();
    assert!(!counts_dir(&scratch).exists());
    assert_eq!(counts.queued_days().len(), 0);
}

#[test]
fn the_sender_reads_the_effective_value_before_every_post() {
    let scratch = Scratch::new("policy-post");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    let upload = FakeUpload::answering(&[Ok(200)]);
    let now = at(4, 600);
    counts.tick(now).unwrap();
    // On when the sender looks for a POST, off by the time it would make it.
    let reads = AtomicUsize::new(0);
    let effective = || reads.fetch_add(1, Ordering::Relaxed) == 0;
    futures::executor::block_on(send_due(&counts, &upload, now, &effective));
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    assert_eq!(upload.seen().len(), 0);
    assert_eq!(counts.mode(), Mode::Off);
    assert!(!counts_dir(&scratch).exists());
}

#[test]
fn a_file_that_does_not_read_as_its_type_is_removed_and_never_sent() {
    let scratch = Scratch::new("bad-file");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 2);
    drop(counts);
    let queue = counts_dir(&scratch).join("queue");
    let report = std::fs::read_to_string(queue.join("2026-11-02.json")).unwrap();
    let planted = report.replace("\"cold\"", "\"/Users/priya/Mehra divorce\"");
    plant(&queue.join("2026-11-02.json"), planted.as_bytes());
    plant(&queue.join("2026-11-05.json"), report.as_bytes());
    plant(
        &counts_dir(&scratch).join("today.json"),
        b"{\"matter\": \"x\"}",
    );
    let counts = saving(&scratch);
    counts.tick(at(3, 60)).unwrap();
    assert_eq!(counts.queued_days().len(), 0);
    assert_eq!(files(&queue).len(), 0);
    let upload = FakeUpload::answering(&[Ok(200)]);
    run(&counts, &upload, at(3, 120), at(4, 0), 60);
    assert_eq!(upload.seen().len(), 0);
}
