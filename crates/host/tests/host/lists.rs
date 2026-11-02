//! The matter list and the thread lists over `subscribe_app` at the interface: the first frames
//! are the whole state, each matter command sends its row (a forget its removal), a thread's
//! change sends its row and its matter's, a streaming turn sends a handful of rows and none per
//! chunk, `MarkSeen` flips `seen`, and a lagged subscriber gets the whole lists again before
//! partial rows resume.

use clauseo_protocol::{
    CommandOutcome, ErrorCode, FieldChange, HostCommand, HostEvent, MatterId, MatterKind,
    MatterStatus, MattersChanged, StopReason, StreamId, ThreadId, ThreadsChanged,
};

use crate::settings::{ready, subscribe_app};
use crate::support::{Dir, FAKE, Running, Scenario, foreign_uuid, matter_with_thread};

/// The rows of every `MattersChanged` in `events`.
fn matters_changed(events: &[HostEvent]) -> Vec<&MattersChanged> {
    events
        .iter()
        .filter_map(|event| match event {
            HostEvent::MattersChanged(changed) => Some(changed),
            _ => None,
        })
        .collect()
}

/// Every `ThreadsChanged` in `events`.
fn threads_changed(events: &[HostEvent]) -> Vec<&ThreadsChanged> {
    events
        .iter()
        .filter_map(|event| match event {
            HostEvent::ThreadsChanged(changed) => Some(changed),
            _ => None,
        })
        .collect()
}

/// The partial `MattersChanged` that sends `matter`'s row as `ListMatters` gives it now.
fn row_of(host: &Running, matter: MatterId) -> HostEvent {
    let row = host
        .matters()
        .into_iter()
        .find(|row| row.matter == matter)
        .unwrap_or_else(|| panic!("{matter} is not listed"));
    HostEvent::MattersChanged(MattersChanged {
        matters: vec![row],
        removed: Vec::new(),
        all: false,
    })
}

/// The partial `ThreadsChanged` that sends `thread`'s row as `ListThreads` gives it now.
fn thread_row_of(host: &Running, matter: MatterId, thread: ThreadId) -> HostEvent {
    let row = host
        .threads(matter)
        .into_iter()
        .find(|row| row.thread == thread)
        .unwrap_or_else(|| panic!("{thread} is not listed"));
    HostEvent::ThreadsChanged(ThreadsChanged {
        matter,
        threads: vec![row],
        removed: Vec::new(),
        all: false,
    })
}

/// The changes a command sent to `sub`, after it is answered.
fn after(host: &Running, sub: &mut clauseo_host::AppSubscription) -> Vec<HostEvent> {
    host.settle();
    ready(sub)
}

#[test]
fn the_first_frames_are_the_settings_and_every_list_whole() {
    let dir = Dir::new(
        "lists_first_frames",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let (mut sub, first) = subscribe_app(&host);
    let matters = host.matters();
    let scratch = matters
        .iter()
        .find(|row| row.kind == MatterKind::Scratch)
        .unwrap()
        .matter;
    let [
        HostEvent::SettingsChanged(_),
        HostEvent::MattersChanged(list),
        HostEvent::ThreadsChanged(scratch_threads),
        HostEvent::ThreadsChanged(matter_threads),
    ] = first.as_slice()
    else {
        panic!("not the four first frames: {first:?}");
    };
    assert_eq!(
        *list,
        MattersChanged {
            matters,
            removed: Vec::new(),
            all: true,
        }
    );
    // One whole list per matter in the list's order, the empty one included.
    assert_eq!(
        *scratch_threads,
        ThreadsChanged {
            matter: scratch,
            threads: Vec::new(),
            removed: Vec::new(),
            all: true,
        }
    );
    assert_eq!(
        *matter_threads,
        ThreadsChanged {
            matter,
            threads: host.threads(matter),
            removed: Vec::new(),
            all: true,
        }
    );
    assert_eq!(after(&host, &mut sub), [], "nothing more until a change");
    host.shutdown();
}

#[test]
fn each_matter_command_sends_its_row_and_a_forget_its_removal() {
    let dir = Dir::new(
        "lists_matter_commands",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (mut sub, _) = subscribe_app(&host);
    let folder = dir.folder("m", &[("a.txt", "x\n")]);

    let matter = host.create_matter("m", &folder);
    assert_eq!(after(&host, &mut sub), [row_of(&host, matter)], "a create");
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        pinned: Some(true),
    });
    let sent = after(&host, &mut sub);
    assert_eq!(sent, [row_of(&host, matter)], "an update");
    assert!(matters_changed(&sent)[0].matters[0].pinned);
    // The same update again changes nothing, so it sends nothing.
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        pinned: Some(true),
    });
    assert_eq!(after(&host, &mut sub), [], "no change");
    host.ok(HostCommand::OpenMatter { matter });
    let sent = after(&host, &mut sub);
    assert_eq!(sent, [row_of(&host, matter)], "an opening");
    assert!(matters_changed(&sent)[0].matters[0].last_opened.is_some());

    // A new thread changes the matter's activity alone: only the thread's row goes.
    let thread = host.create_thread(matter, FAKE);
    assert_eq!(
        after(&host, &mut sub),
        [thread_row_of(&host, matter, thread)],
        "a new thread"
    );

    host.ok(HostCommand::ForgetMatter { matter });
    assert_eq!(
        after(&host, &mut sub),
        [HostEvent::MattersChanged(MattersChanged {
            matters: Vec::new(),
            removed: vec![matter],
            all: false,
        })],
        "a forget, with no ThreadsChanged"
    );
    // Importing the folder again brings it back: its row, then its threads as new rows.
    assert_eq!(host.create_matter("m", &folder), matter);
    assert_eq!(
        after(&host, &mut sub),
        [row_of(&host, matter), thread_row_of(&host, matter, thread)],
        "a bring-back"
    );
    // A refused command changes nothing and sends nothing.
    host.refused(
        HostCommand::MarkSeen {
            thread: ThreadId::new(foreign_uuid(9)).unwrap(),
        },
        ErrorCode::CommandUnknownThread,
    );
    assert_eq!(after(&host, &mut sub), []);
    host.shutdown();
}

/// The most app events one streaming turn may send: its matter working with its thread starting,
/// its thread running, then its matter ready with its thread idle.
const TURN_EVENTS: usize = 5;

/// The fewest envelopes the turn logs: twenty tool call updates and more.
const TURN_ENVELOPES: usize = 20;

#[test]
fn a_streaming_turn_sends_a_handful_of_rows_and_mark_seen_one_more_pair() {
    let dir = Dir::new(
        "lists_streaming_turn",
        &[(FAKE, Scenario::Bundled("tool-calls"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let (mut sub, _) = subscribe_app(&host);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the folder.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    let sent = after(&host, &mut sub);
    let logged = events.seen.len();
    eprintln!(
        "a streaming turn: {logged} envelopes logged, {} app events sent",
        sent.len()
    );
    assert!(
        !sent.is_empty() && sent.len() <= TURN_EVENTS,
        "{} app events for {logged} envelopes: {sent:?}",
        sent.len()
    );
    assert!(
        logged >= TURN_ENVELOPES,
        "the turn logged only {logged} envelopes"
    );
    assert!(
        sent.iter().all(|event| match event {
            HostEvent::MattersChanged(changed) => !changed.all,
            HostEvent::ThreadsChanged(changed) => !changed.all,
            _ => false,
        }),
        "only partial rows: {sent:?}"
    );
    // What the rail ends on: the answer ready, the thread not seen, with its stop.
    let last_matter = matters_changed(&sent).last().unwrap().matters[0].clone();
    assert_eq!(last_matter.status, MatterStatus::Ready);
    let last_thread = threads_changed(&sent).last().unwrap().threads[0].clone();
    assert!(!last_thread.seen);
    let stop = last_thread.last_stop.unwrap();
    assert_eq!(stop.reason, StopReason::EndTurn);
    assert!(
        last_thread.last_activity >= Some(stop.at),
        "{last_thread:?}"
    );
    assert_eq!(last_thread, host.threads(matter).remove(0));

    // Seeing it sends both rows once: the matter idle, the thread seen.
    assert_eq!(
        host.ok(HostCommand::MarkSeen { thread }),
        CommandOutcome::Done
    );
    let sent = after(&host, &mut sub);
    assert_eq!(
        sent,
        [row_of(&host, matter), thread_row_of(&host, matter, thread)]
    );
    assert_eq!(
        matters_changed(&sent)[0].matters[0].status,
        MatterStatus::Idle
    );
    assert!(threads_changed(&sent)[0].threads[0].seen);
    // Seeing it again changes nothing.
    host.ok(HostCommand::MarkSeen { thread });
    assert_eq!(after(&host, &mut sub), []);
    host.shutdown();
}

/// The capacity of the lagging subscriber's channel.
const SMALL_CAPACITY: usize = 4;

#[test]
fn a_lagged_subscriber_gets_the_whole_lists_then_partial_rows() {
    let dir = Dir::new(
        "lists_lagged",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_app_subscription_capacity(SMALL_CAPACITY)
    });
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    // The first frames (settings, matters, two thread lists) fill the four places exactly.
    let (mut sub, first) = subscribe_app(&host);
    assert_eq!(first.len(), SMALL_CAPACITY);
    // Five changes: four fill the channel, the fifth finds it full.
    let pins = [true, false, true, false, true];
    for pinned in pins {
        host.ok(HostCommand::UpdateMatter {
            matter,
            client: FieldChange::Keep,
            next_date: FieldChange::Keep,
            pinned: Some(pinned),
        });
    }
    host.settle();
    let partials = ready(&mut sub);
    let seen: Vec<bool> = matters_changed(&partials)
        .iter()
        .map(|changed| {
            assert!(!changed.all);
            changed.matters[0].pinned
        })
        .collect();
    assert_eq!(seen, pins[..SMALL_CAPACITY]);
    // Drained, it gets the whole state at once instead of the missed row.
    let caught = crate::support::wait("the catch-up", async {
        use futures::StreamExt as _;
        let mut caught = Vec::new();
        while caught.len() < SMALL_CAPACITY {
            let item = sub.next().await.expect("the app stream ended");
            caught.push(crate::settings::decode(&item));
        }
        caught
    });
    let [
        HostEvent::SettingsChanged(_),
        HostEvent::MattersChanged(list),
        HostEvent::ThreadsChanged(one),
        HostEvent::ThreadsChanged(two),
    ] = caught.as_slice()
    else {
        panic!("not the whole state: {caught:?}");
    };
    assert!(list.all && one.all && two.all);
    assert_eq!(list.matters, host.matters());
    assert!(
        list.matters
            .iter()
            .any(|row| row.matter == matter && row.pinned)
    );
    // Then partial rows resume.
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        pinned: Some(false),
    });
    assert_eq!(after(&host, &mut sub), [row_of(&host, matter)]);
    host.shutdown();
}
