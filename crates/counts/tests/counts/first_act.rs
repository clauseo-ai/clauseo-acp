//! The first act (research 37 §4.2), off and on again.

use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use clauseo_counts::{Counts, Mode, Upload, UploadError, UploadPath, send_due};
use futures::future::BoxFuture;

use crate::support::*;

fn before_first_act(scratch: &Scratch) -> Counts {
    Counts::open(scratch.path(), about(), true, false, SEED)
}

#[test]
fn before_the_first_act_cells_live_in_memory_and_nothing_is_written_or_sent() {
    let scratch = Scratch::new("memory");
    let counts = before_first_act(&scratch);
    assert_eq!(counts.mode(), Mode::Memory);
    fill_days(&counts, 2, 4);
    counts.bump(COLD);
    assert_eq!(counts.queued_days(), vec![day(2), day(3), day(4)]);
    assert_eq!(counts.today().app_opened.len(), 1);
    let upload = FakeUpload::answering(&[Ok(200)]);
    run(&counts, &upload, at(5, 60), at(6, 0), 60);
    assert_eq!(upload.seen().len(), 0);
    assert!(
        files(scratch.path()).is_empty(),
        "{:?}",
        files(scratch.path())
    );
}

#[test]
fn the_first_act_writes_the_marker_makes_the_id_and_starts_saving() {
    let scratch = Scratch::new("first-act");
    let counts = before_first_act(&scratch);
    fill_days(&counts, 2, 3);
    counts.bump(COLD);
    counts.first_act(true).unwrap();
    assert_eq!(counts.mode(), Mode::Saving);
    let id = counts.install_id().unwrap();
    assert_eq!(
        files(scratch.path()),
        [
            "counts/install_id",
            "counts/queue/2026-11-02.json",
            "counts/queue/2026-11-03.json",
            "counts/tally/2026-W45.json",
            "counts/today.json",
            "first_act_done",
        ]
    );
    let saved = std::fs::read_to_string(counts_dir(&scratch).join("install_id")).unwrap();
    assert_eq!(saved, id.to_string());

    // A second first act changes nothing, whatever it says.
    counts.first_act(false).unwrap();
    assert_eq!(counts.mode(), Mode::Saving);
    assert_eq!(counts.install_id(), Some(id));
    assert_eq!(files(scratch.path()).len(), 6);

    // Reopened, it reads everything back.
    let counts = saving(&scratch);
    counts.tick(at(4, 60)).unwrap();
    assert_eq!(counts.install_id(), Some(id));
    assert_eq!(counts.queued_days(), vec![day(2), day(3)]);
    assert_eq!(counts.today().app_opened[0].n.get(), 1);
}

#[test]
fn a_first_act_with_counts_off_writes_only_the_marker() {
    let scratch = Scratch::new("first-act-off");
    let counts = before_first_act(&scratch);
    fill_days(&counts, 2, 3);
    counts.first_act(false).unwrap();
    assert_eq!(counts.mode(), Mode::Off);
    assert_eq!(files(scratch.path()), ["first_act_done"]);
    assert_eq!(counts.queued_days().len(), 0);
}

/// An `Upload` whose POST never answers, so it stays in flight until cancelled.
struct Hanging(Mutex<usize>);

impl Upload for Hanging {
    fn post(&self, _: UploadPath, _: Vec<u8>) -> BoxFuture<'_, Result<u16, UploadError>> {
        *self.0.lock().unwrap() += 1;
        Box::pin(futures::future::pending())
    }
}

#[test]
fn off_deletes_the_folder_and_cancels_the_post_in_flight_without_retrying_it() {
    let scratch = Scratch::new("off");
    let counts = saving(&scratch);
    fill_days(&counts, 2, 3);
    let old_id = counts.install_id().unwrap();
    let upload = Hanging(Mutex::new(0));
    let now = at(4, 600);
    counts.tick(now).unwrap();
    let mut sending = pin!(send_due(&counts, &upload, now, &|| true));
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    assert!(sending.as_mut().poll(&mut cx).is_pending());
    assert_eq!(*upload.0.lock().unwrap(), 1);

    counts.set_enabled(false).unwrap();
    assert_eq!(sending.as_mut().poll(&mut cx), Poll::Ready(()));
    assert_eq!(counts.mode(), Mode::Off);
    assert_eq!(files(scratch.path()).len(), 0);
    // Nothing is counted while off.
    counts.bump(COLD);
    counts.tick(at(4, 700)).unwrap();
    assert_eq!(counts.today().app_opened.len(), 0);
    assert!(!counts_dir(&scratch).exists());

    // On again: a new id, an empty queue, and the cancelled days never go.
    counts.set_enabled(true).unwrap();
    let new_id = counts.install_id().unwrap();
    assert_ne!(new_id, old_id);
    assert_eq!(
        std::fs::read_to_string(counts_dir(&scratch).join("install_id")).unwrap(),
        new_id.to_string()
    );
    let answering = FakeUpload::answering(&[Ok(200)]);
    let sent = run(&counts, &answering, at(4, 800), at(5, DAY - 1), 60);
    let days = sent
        .iter()
        .filter(|(_, seen)| seen.path == UploadPath::Counts)
        .count();
    assert_eq!(days, 0, "{sent:?}");
}
