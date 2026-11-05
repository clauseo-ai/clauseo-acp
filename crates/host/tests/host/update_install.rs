//! Installs that wait on turns (decision record 0037, R23 PR 4): `after_turn` waits until no
//! thread has a running turn, `now` stops every running turn as Stop does and holds new sends
//! queued until the install goes ahead or is taken back, and the host then stops and ends with
//! `HostExit::Install`. Also the download's side: dropping it cancels it, and a download that
//! floods progress does not stall the host.

use std::sync::atomic::Ordering;

use clauseo_counts::schema::{Update, UpdateResult};
use clauseo_host::InstallKind;
use clauseo_protocol::{
    CommandOutcome, Count, DeliveryRequest, HostCommand, InstallWhen, StopReason, StreamId,
    UpdateState,
};

use crate::counts::open_counts;
use crate::observe::started;
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{DURABLE, send};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, text};
use crate::update::{
    CURRENT, Download, FakeUpdater, INSTALLING, NEXT, assert_installs, exit_of, marker_text,
    start_updating, start_updating_with, until_ready, until_view, view,
};

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
