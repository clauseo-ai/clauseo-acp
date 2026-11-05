//! Installs that wait on turns (decision record 0037, R23 PR 4): `after_turn` waits until no
//! thread has a running turn, `now` stops every running turn as Stop does and holds new sends
//! queued until the install goes ahead or is taken back, and the host then stops and ends with
//! `HostExit::Install`. Also the download's side: dropping it cancels it, and a download that
//! floods progress does not stall the host.

use std::sync::atomic::Ordering;

use clauseo_counts::schema::{Update, UpdateResult};
use clauseo_host::{HostConfig, HostExit, InstallKind};
use clauseo_protocol::{
    CommandOutcome, Count, DeliveryRequest, FailedStage, HostCommand, InstallWhen, StopReason,
    StreamId, UpdateState,
};

use crate::counts::open_counts;
use crate::observe::started;
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{CANCEL_WAIT, DURABLE, held, send};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, text, wait};
use crate::update::{
    CURRENT, Download, FakeUpdater, INSTALLING, NEXT, assert_installs, exit_of, marker_text,
    start_updating, start_updating_with, until_ready, until_view, view,
};
use crate::write;

/// A second agent id, for a thread whose agent plays another scenario.
const OTHER: &str = "fake-other";

fn install(when: InstallWhen) -> HostCommand {
    HostCommand::InstallUpdate { when }
}

fn scheduled() -> UpdateState {
    UpdateState::Scheduled {
        version: NEXT.to_owned(),
    }
}

fn installing() -> UpdateState {
    UpdateState::Installing {
        version: NEXT.to_owned(),
    }
}

#[test]
fn after_turn_waits_until_no_thread_has_a_running_turn() {
    let dir = Dir::new(
        "update_after_turn",
        &[(FAKE, Scenario::Local("update-hold"))],
    );
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::offering(&dir);
    let counted = counts.clone();
    let host = start_updating_with(&dir, 0, &fake, CURRENT, move |config| {
        config.with_counts(counted)
    });
    let (_, one, _) = matter_with_thread(&dir, &host, "one", &[], FAKE);
    let (_, two, _) = matter_with_thread(&dir, &host, "two", &[], FAKE);
    let mut first = host.events(StreamId::Thread(one));
    let mut second = host.events(StreamId::Thread(two));
    let (turn_one, _) = host.prompt(one, "Edit clause 1.");
    let (turn_two, _) = host.prompt(two, "Edit clause 1.");
    until_first_request(&mut first);
    until_first_request(&mut second);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);

    // Taken back, then asked again.
    assert_eq!(
        host.ok(install(InstallWhen::AfterTurn)),
        CommandOutcome::Done
    );
    assert_eq!(view(&host).state, scheduled());
    host.ok(HostCommand::CancelScheduledUpdate {});
    until_ready(&host);
    host.ok(install(InstallWhen::AfterTurn));
    assert_eq!(view(&host).state, scheduled());

    // One thread's turn ends: the other still runs.
    answer_from(&host, &mut first, one, 0, &["allow-once"]);
    assert_eq!(first.until_turn_ended(turn_one).stop, StopReason::EndTurn);
    host.settle();
    assert_eq!(view(&host).state, scheduled());
    assert_eq!(marker_text(&dir), None);

    // A new turn starts meanwhile (nothing is held under `after_turn`) and pushes the install
    // back past the other thread's end.
    let (again, _) = host.prompt(one, "Edit clause 2.");
    first.until("the second turn's request", |e| {
        e.permission_requests().len() > 1
    });
    answer_from(&host, &mut second, two, 0, &["allow-once"]);
    assert_eq!(second.until_turn_ended(turn_two).stop, StopReason::EndTurn);
    host.settle();
    assert_eq!(view(&host).state, scheduled());
    assert_eq!(marker_text(&dir), None);

    // The last one ends: the host stops to install.
    answer_from(&host, &mut first, one, 1, &["allow-once"]);
    assert_eq!(first.until_turn_ended(again).stop, StopReason::EndTurn);
    let path = dir.data().join("updates").join(NEXT).join("Clauseo.exe");
    assert_installs(&exit_of(host), &path);
    assert_eq!(marker_text(&dir).as_deref(), Some(INSTALLING));
    let after_answer = Update {
        result: UpdateResult::AfterAnswer,
    };
    let counted: Vec<_> = counts
        .today()
        .update
        .iter()
        .map(|cell| (cell.key, u64::from(cell.n)))
        .collect();
    assert_eq!(counted, [(after_answer, 1)]);
}

#[test]
fn install_now_stops_running_turns_then_installs() {
    let dir = Dir::new(
        "update_now",
        &[(FAKE, Scenario::Bundled("permission-cancelled"))],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);

    assert_eq!(host.ok(install(InstallWhen::Now)), CommandOutcome::Done);
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Cancelled);
    let path = dir.data().join("updates").join(NEXT).join("Clauseo.exe");
    assert_installs(&exit_of(host), &path);
    assert_eq!(marker_text(&dir).as_deref(), Some(INSTALLING));

    // The next start is the new version: the store closed cleanly with the cancel in it.
    let quiet = FakeUpdater::new(&dir, InstallKind::CurrentUser, None, Download::Instant);
    let host = start_updating_with(&dir, 1, &quiet, NEXT, |config| config);
    assert_eq!(view(&host).updated_from.as_deref(), Some(CURRENT));
    let mut replay = host.events(StreamId::Thread(thread));
    replay.until_caught_up();
    assert_eq!(
        replay.turn_ended(turn).map(|ended| ended.stop),
        Some(StopReason::Cancelled)
    );
    assert!(host.matters().iter().any(|m| m.matter == matter));
    host.shutdown();
}

#[test]
fn sends_wait_queued_while_turns_stop_and_start_once_the_install_is_taken_back() {
    let dir = Dir::new(
        "update_gate",
        &[
            (FAKE, Scenario::Bundled("cancel-unanswered")),
            (OTHER, Scenario::Bundled("prompt-stream")),
        ],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (_, stuck, _) = matter_with_thread(&dir, &host, "stuck", &[], FAKE);
    let (_, other, _) = matter_with_thread(&dir, &host, "other", &[], OTHER);
    let mut others = host.events(StreamId::Thread(other));
    host.prompt(stuck, "Draft clause 4.");
    // The agent's first chunk is in: the turn is prompting, and it never answers a cancel.
    host.wait_armed(DURABLE);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);

    host.ok(install(InstallWhen::Now));
    assert_eq!(view(&host).state, installing());
    // Whatever its delivery, a send waits queued and does not start.
    let (held, _) = send(&host, other, text("Summarise."), DeliveryRequest::SendNow);
    host.settle();
    others.drain_ready();
    assert!(others.last_queued(held).is_some(), "{}", others.dump());
    assert_eq!(
        others.position(|e| started(e, held)),
        None,
        "{}",
        others.dump()
    );

    // Taken back: `ready` again, and the held send starts and ends.
    assert_eq!(
        host.ok(HostCommand::CancelScheduledUpdate {}),
        CommandOutcome::Done
    );
    assert_eq!(
        view(&host).state,
        UpdateState::Ready {
            version: NEXT.to_owned()
        }
    );
    assert_eq!(others.until_turn_ended(held).stop, StopReason::EndTurn);
    host.shutdown_past_cancel();
}

#[test]
fn dropping_the_download_cancels_it() {
    let dir = Dir::new("update_download_drop", &[]);
    let fake = FakeUpdater::new(&dir, InstallKind::CurrentUser, Some(NEXT), Download::Hang);
    let host = start_updating(&dir, 0, &fake);
    host.ok(HostCommand::CheckForUpdate {});
    until_view(&host, "downloading", |view| {
        matches!(view.state, UpdateState::Downloading { .. })
    });
    assert_eq!(fake.downloads(), 1);
    assert!(!fake.dropped.load(Ordering::SeqCst));
    host.shutdown();
    assert!(fake.dropped.load(Ordering::SeqCst));
}

#[test]
fn a_download_that_floods_progress_does_not_stall_the_host() {
    let dir = Dir::new("update_flood", &[]);
    let fake = FakeUpdater::new(&dir, InstallKind::CurrentUser, Some(NEXT), Download::Flood);
    let host = start_updating(&dir, 0, &fake);
    host.ok(HostCommand::CheckForUpdate {});
    until_view(
        &host,
        "downloading with progress",
        |view| matches!(view.state, UpdateState::Downloading { received, .. } if received != Count::ZERO),
    );
    // The host answers while the flood goes on.
    for n in 0..20 {
        let folder = dir.folder(&format!("m{n}"), &[]);
        host.create_matter(&format!("m{n}"), &folder);
    }
    assert!(host.matters().len() > 20);
    fake.stop_flood.store(true, Ordering::SeqCst);
    until_ready(&host);
    host.shutdown();
}

#[test]
fn a_send_now_held_on_the_stopped_thread_starts_once_the_install_is_taken_back() {
    // Review of R23 PR 4: the held send keeps its intent; without the install it would have
    // started after the turn it stopped, whatever that turn's stop.
    let dir = Dir::new(
        "update_gate_same_thread",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Draft clause 4.");
    host.wait_armed(DURABLE);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);
    host.ok(install(InstallWhen::Now));
    let (later, _) = send(&host, thread, text("Summarise."), DeliveryRequest::SendNow);
    host.ok(HostCommand::CancelScheduledUpdate {});
    host.wait_armed(CANCEL_WAIT);
    host.clock.advance(CANCEL_WAIT);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    // It left the queue and went to the agent (which, its script done, has exited): a turn
    // taken out of the queue unstarted would end `Cancelled`.
    let ended = events.until_turn_ended(later);
    assert_ne!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    assert!(
        !held(&host, matter, thread),
        "held after the install was taken back"
    );
    host.shutdown_past_cancel();
}

#[test]
fn a_marker_that_cannot_be_written_fails_the_install_and_lifts_the_hold() {
    let dir = Dir::new(
        "update_marker_unwritable",
        &[
            (FAKE, Scenario::Bundled("permission-cancelled")),
            (OTHER, Scenario::Bundled("prompt-stream")),
        ],
    );
    // A folder where `update.json` goes: the rename onto it fails.
    write::write(&dir.data().join("update.json").join("in-the-way"), b"x");
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::offering(&dir);
    let counted = counts.clone();
    let host = start_updating_with(&dir, 0, &fake, CURRENT, move |config| {
        config.with_counts(counted)
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let (_, other, _) = matter_with_thread(&dir, &host, "other", &[], OTHER);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);

    host.ok(install(InstallWhen::Now));
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Cancelled);
    let failed = until_view(&host, "failed", |view| {
        matches!(view.state, UpdateState::Failed { .. })
    });
    assert!(
        matches!(
            &failed.state,
            UpdateState::Failed { version: Some(v), stage: FailedStage::Install, .. } if v == NEXT
        ),
        "{failed:?}"
    );
    // Nothing is held any more: a send starts and ends.
    let mut others = host.events(StreamId::Thread(other));
    let (next, _) = host.prompt(other, "Summarise.");
    assert_eq!(others.until_turn_ended(next).stop, StopReason::EndTurn);
    host.settle();
    let counted: Vec<_> = counts
        .today()
        .update
        .iter()
        .map(|cell| (cell.key, u64::from(cell.n)))
        .collect();
    let failure = Update {
        result: UpdateResult::Failed,
    };
    assert_eq!(counted, [(failure, 1)]);
    host.shutdown();
}

#[test]
fn an_install_whose_close_failed_stops_and_leaves_no_marker() {
    // The next start then finds the download again, and reports no failed install.
    let dir = Dir::new("update_close_failed", &[]);
    let fake = FakeUpdater::offering(&dir);
    let mut host = start_updating_with(&dir, 0, &fake, CURRENT, HostConfig::with_failing_close);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);
    host.ok(install(InstallWhen::AfterTurn));
    assert_eq!(
        wait("the host's exit", host.handle.exit()),
        HostExit::Stopped
    );
    let joined = host.take_join().join();
    assert!(
        joined.is_err(),
        "the close's failure is the thread's result"
    );
    assert_eq!(marker_text(&dir), None);
}

#[test]
fn a_send_now_held_on_an_idle_thread_moves_its_held_queue_on_once_the_install_is_taken_back() {
    // Review of R23 PR 4: without the install, the send would have started at once ahead of
    // the held queue and let it move on after; it does so once the hold lifts.
    let dir = Dir::new(
        "update_gate_held_queue",
        &[
            (FAKE, Scenario::Bundled("send-now")),
            (OTHER, Scenario::Bundled("cancel-unanswered")),
        ],
    );
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let (_, other, _) = matter_with_thread(&dir, &host, "other", &[], OTHER);
    let mut events = host.events(StreamId::Thread(thread));
    // A queued turn held after a cancel.
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);
    let (waiting, _) = host.prompt(thread, "Then the notice period.");
    host.ok(HostCommand::Cancel { thread });
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    host.settle();
    assert!(held(&host, matter, thread), "the queue is not held");
    // Another thread's turn keeps the install's hold up.
    host.prompt(other, "Draft clause 4.");
    host.wait_armed(DURABLE);
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);
    host.ok(install(InstallWhen::Now));

    let text_now = text("Use the 2019 version instead");
    let (now, _) = send(&host, thread, text_now, DeliveryRequest::SendNow);
    host.ok(HostCommand::CancelScheduledUpdate {});
    assert_eq!(events.until_turn_ended(now).stop, StopReason::EndTurn);
    // The held turn moves on after it (to an agent whose script is done: it does not end
    // `Cancelled`, as a turn taken out of the queue unstarted would).
    let ended = events.until_turn_ended(waiting);
    assert_ne!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    host.shutdown_past_cancel();
}

/// The data folder's mode is set to `mode` until dropped.
#[cfg(unix)]
struct FolderMode<'a>(&'a std::path::Path);

#[cfg(unix)]
impl FolderMode<'_> {
    fn set(path: &std::path::Path, mode: u32) -> FolderMode<'_> {
        write::set_mode(path, mode);
        FolderMode(path)
    }
}

#[cfg(unix)]
impl Drop for FolderMode<'_> {
    fn drop(&mut self) {
        write::set_mode(self.0, 0o700);
    }
}

#[cfg(unix)]
#[test]
fn a_marker_on_disk_whose_folder_sync_failed_still_installs_and_counts_once() {
    // Review of R23 PR 4: once `update.json` is renamed into place, the data folder's sync
    // failing is not a failed write.
    let dir = Dir::new("update_marker_sync_failed", &[]);
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::offering(&dir);
    let counted = counts.clone();
    let host = start_updating_with(&dir, 0, &fake, CURRENT, move |config| {
        config.with_counts(counted)
    });
    host.ok(HostCommand::CheckForUpdate {});
    until_ready(&host);
    let exit = {
        // Write and search, no read: the rename works and the folder cannot be opened to sync.
        let _mode = FolderMode::set(dir.data(), 0o300);
        host.ok(install(InstallWhen::AfterTurn));
        wait("the host's exit", host.handle.exit())
    };
    let path = dir.data().join("updates").join(NEXT).join("Clauseo.exe");
    assert_installs(&exit_of_exited(host, exit), &path);
    assert_eq!(marker_text(&dir).as_deref(), Some(INSTALLING));

    // The install did not take: the next start counts one failure, the only one.
    let counted = counts.clone();
    let quiet = FakeUpdater::new(&dir, InstallKind::CurrentUser, None, Download::Instant);
    let host = start_updating_with(&dir, 1, &quiet, CURRENT, move |config| {
        config.with_counts(counted)
    });
    host.settle();
    let failed: Vec<_> = counts
        .today()
        .update
        .iter()
        .filter(|cell| cell.key.result == UpdateResult::Failed)
        .map(|cell| u64::from(cell.n))
        .collect();
    assert_eq!(failed, [1]);
    host.shutdown();
}

/// Joins `host`, whose exit was `exit`.
#[cfg(unix)]
fn exit_of_exited(mut host: crate::support::Running, exit: HostExit) -> HostExit {
    host.take_join()
        .join()
        .unwrap_or_else(|e| panic!("join: {e}"));
    exit
}
