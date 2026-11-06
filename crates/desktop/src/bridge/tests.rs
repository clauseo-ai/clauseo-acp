//! The bridge against a real host on a scratch folder: what reaches the window's channel, and
//! what the commands refuse. No agent runs.

use std::path::PathBuf;
use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, ConfigValue, Cursor, ErrorCode, HostEvent, PatchBatch, PermissionRequestId,
    StreamHeader, Title, ViewEpoch, ViewPatchBody,
};
use uuid::Uuid;

use super::*;
use crate::dialogs::BoxFuture;
use crate::test_support::{Messages, Running, Scratch, block_on, envelope, open, stream};

const MAIN: &str = "main";
const SETTINGS: &str = "settings";

/// A picker that returns one folder.
struct Picks(Option<PathBuf>);

impl Picker for Picks {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>> {
        let folder = self.0.clone();
        Box::pin(async move { folder })
    }
}

/// The batch inside `event`.
fn batch(event: HostEvent) -> PatchBatch {
    let HostEvent::Patches(batch) = event else {
        panic!("not patches: {event:?}");
    };
    batch
}

/// A model of what the webview does with a stream's patches (ui's `view.test.ts` checks the real
/// one): a `Reset` replaces the view, a patch applies only at the next cursor, and any other
/// cursor is a replay that changes nothing.
#[derive(Default)]
struct Applier {
    applied: Option<(ViewEpoch, Cursor)>,
    patches_applied: u32,
    replays: u32,
}

impl Applier {
    fn apply(&mut self, batch: &PatchBatch) {
        for patch in &batch.patches {
            if let ViewPatchBody::Reset { .. } = patch.body {
                self.applied = Some((batch.epoch, patch.cursor));
                continue;
            }
            let next = self
                .applied
                .filter(|(epoch, _)| *epoch == batch.epoch)
                .map(|(_, cursor)| cursor.get() + 1);
            if next == Some(patch.cursor.get()) {
                self.applied = Some((batch.epoch, patch.cursor));
                self.patches_applied += 1;
            } else {
                self.replays += 1;
            }
        }
    }
}

#[test]
fn a_stale_cursor_gets_reset() {
    let scratch = Scratch::new("stale-cursor", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let stream = StreamId::Matter(host.scratch());

    // An epoch the host never minted: the position is stale.
    let stale = ViewPosition {
        epoch: ViewEpoch::new(Uuid::now_v7()).unwrap(),
        cursor: Cursor::new(3).unwrap(),
    };
    block_on(bridge.subscribe(MAIN, stream, Some(stale))).unwrap();
    let batch = batch(messages.next_event());
    assert_eq!(batch.stream, stream);
    assert_ne!(batch.epoch, stale.epoch);
    assert!(
        matches!(batch.patches.as_slice(), [only] if matches!(only.body, ViewPatchBody::Reset { .. })),
        "not one Reset: {batch:?}"
    );
}

/// The e2e build sends an event that fits wrapped as `timed`, with the time the bridge received
/// it, and counts only the event's length in `perf_channel_bytes`'s total, what release sends:
/// never the wrapper. The default build sends it plain.
#[test]
fn only_the_e2e_build_sends_an_event_timed_and_counts_its_bytes() {
    let scratch = Scratch::new("timed-event", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    #[cfg(feature = "e2e")]
    let (before, start) = (crate::e2e::channel_bytes(), crate::e2e::epoch_ms());
    block_on(bridge.subscribe(MAIN, StreamId::Matter(host.scratch()), None)).unwrap();
    let event = messages.next_text();
    let _reset = batch(serde_json::from_str(&event).unwrap());
    let timed = messages.timed();
    #[cfg(not(feature = "e2e"))]
    assert!(timed.is_empty(), "{timed:?}");
    #[cfg(feature = "e2e")]
    check_timed(&timed, before, start, event.len());
}

/// One timed message, received after `start`, its event of `event_len` bytes counted in the
/// total past `before` and its wrapper not.
#[cfg(feature = "e2e")]
fn check_timed(timed: &[(f64, usize)], before: u64, start: f64, event_len: usize) {
    let [(received_ms, wrapped_len)] = timed else {
        panic!("{timed:?}");
    };
    assert!(start <= *received_ms && *received_ms <= crate::e2e::epoch_ms());
    assert!(*wrapped_len > event_len);
    // nextest runs each test in its own process, so only this test's message is in the total.
    assert_eq!(crate::e2e::channel_bytes() - before, event_len as u64);
}

#[test]
fn a_resubscribe_from_an_older_cursor_gets_the_tail_in_the_same_epoch() {
    let scratch = Scratch::new("older-cursor", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let stream = stream(thread);
    let mut view = Applier::default();

    block_on(bridge.subscribe(MAIN, stream, None)).unwrap();
    view.apply(&batch(messages.next_event()));
    let (epoch, reset_at) = view.applied.unwrap();

    host.rename(thread, "One");
    view.apply(&batch(messages.next_event()));
    host.rename(thread, "Two");
    view.apply(&batch(messages.next_event()));
    assert_eq!((view.patches_applied, view.replays), (2, 0));

    // Restarting from an older position (as a lagging ack would) replays the second rename,
    // and the webview applies it no second time.
    let older = ViewPosition {
        epoch,
        cursor: Cursor::new(reset_at.get() + 1).unwrap(),
    };
    block_on(bridge.subscribe(MAIN, stream, Some(older))).unwrap();
    let replay = batch(messages.next_event());
    assert_eq!(replay.epoch, epoch, "a Reset, not the tail: {replay:?}");
    view.apply(&replay);
    assert_eq!((view.patches_applied, view.replays), (2, 1));

    host.rename(thread, "Three");
    view.apply(&batch(messages.next_event()));
    assert_eq!((view.patches_applied, view.replays), (3, 1));
    assert_eq!(
        view.applied.map(|(_, cursor)| cursor.get()),
        Some(reset_at.get() + 3)
    );
}

/// Renames `thread`, followed on `MAIN` of `bridge` from its first `Reset`, until the window
/// pauses, and returns the stream's messages sent: the `Reset` and every rename's. With
/// `fire_at`, the watchdog fires after that many renames, as on a runner slow enough to go
/// [`window::WATCHDOG`] without an ack while it fills the window: every stream and feed restarts,
/// and the limits feed sends its view again.
///
/// The window's own watchdog is stopped first, so only `fire_at` fires it. Left running, it fires
/// on the wall clock on any run that takes [`window::WATCHDOG`] from `open`, and a fire between
/// the pause and the caller's ack restarts every subscription, sends one message more and pauses
/// again: the caller's ack then misses that message, and what it reads next depends on which
/// restart the host answered first.
///
/// The window counts every message toward its cap, the limits messages too: the limits view
/// `open` waited for, and the one each watchdog fire resends. So the bytes the page received are
/// the stream's messages' and the limits messages' together, and they must be the window's
/// count exactly, and past the cap.
fn fill_until_paused(
    host: &Running,
    bridge: &Bridge,
    messages: &Messages,
    thread: ThreadId,
    fire_at: Option<u32>,
) -> Vec<String> {
    let window = bridge.window(MAIN).unwrap();
    block_on(pump::stop_watchdog(&window));
    block_on(bridge.subscribe(MAIN, stream(thread), None)).unwrap();
    let mut sent = vec![messages.next_text()];

    // Long titles, so each rename's patch is large: renames until the window pauses.
    let long = "x".repeat(150);
    let mut renames = 0;
    while !lock(&window).state().is_paused() {
        renames += 1;
        assert!(renames < 5000, "never paused");
        host.rename(thread, &format!("{renames} {long}"));
        sent.push(messages.next_text());
        if Some(renames) == fire_at {
            // Far past the watchdog's wait, whatever the window's clock reads. The restart's
            // `Reset` comes in among the renames' messages, and the loop takes it as one.
            pump::tick_at(&window, Duration::from_secs(3600));
            // The limits view it resends, unless the loop took it already. (On a run that took
            // a full watchdog wait from `open` to stopping the watchdog, that fire resent the
            // view, and this one does not fire: no ack rearmed it.)
            if messages.limits_received() < 2 {
                let _view = messages.next_limits();
            }
        }
    }
    // A rename may make more than one message: take every one sent before the pause. The pump
    // sends while holding the window's lock, so with the lock held every message the window
    // counted is in the channel, and no other comes in.
    let counted = {
        let window = lock(&window);
        sent.extend(messages.drain());
        window.state().unacked_bytes()
    };
    // In the e2e build each message the collector unwrapped was longer by its wrapper.
    let stream_bytes = sent.iter().map(String::len).sum::<usize>() + messages.timed_overhead();
    let received = stream_bytes + messages.limits_bytes();
    assert_eq!(
        received as u64,
        counted,
        "the page got {stream_bytes} stream bytes and {} limits bytes in {} limits messages",
        messages.limits_bytes(),
        messages.limits_received()
    );
    assert!(
        counted > window::UNACKED_CAP,
        "paused at {counted} bytes, of which {stream_bytes} the stream's"
    );
    sent
}

#[test]
fn past_the_cap_the_driver_pauses_and_resubscribes_once_acks_catch_up() {
    let scratch = Scratch::new("cap", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let stream = stream(thread);
    let sent = fill_until_paused(&host, &bridge, &messages, thread, None);
    // The pause dropped the host subscription: a rename now reaches no message.
    host.rename(thread, "while paused");

    // The webview applies everything it got and acks it all: the bridge resubscribes after
    // the last acknowledged cursor, and the host sends the tail from there.
    let last = batch(serde_json::from_str(sent.last().unwrap()).unwrap());
    let cursor = last.patches.last().unwrap().cursor;
    // The limits view sent at open counts too.
    let count = u32::try_from(sent.len()).unwrap() + messages.limits_received();
    let position = Ack {
        stream,
        epoch: last.epoch,
        cursor,
    };
    block_on(bridge.ack(MAIN, vec![position], count)).unwrap();
    // The host answers with the tail after that cursor or, when it unloaded the stream while
    // nobody followed it, a Reset in a new epoch. Either way the rename made while paused shows.
    let next = batch(messages.next_event());
    let first = next.patches.first().unwrap();
    let header = match &first.body {
        ViewPatchBody::Upsert { entry } => {
            assert_eq!(
                (next.epoch, first.cursor.get()),
                (last.epoch, cursor.get() + 1)
            );
            match entry.as_ref() {
                ViewEntry::Header(header) => (**header).clone(),
                other => panic!("not the header: {other:?}"),
            }
        }
        ViewPatchBody::Reset { view_window } => match &view_window.header {
            Some(StreamHeader::Thread(header)) => (**header).clone(),
            other => panic!("no thread header: {other:?}"),
        },
        other => panic!("neither the tail nor a Reset: {other:?}"),
    };
    assert_eq!(header.title.as_deref(), Some("while paused"));
}

/// Issue #307: a watchdog fire while the window fills, as on a runner that takes more than
/// [`window::WATCHDOG`] to make the renames, resends the limits view, which the window counts.
/// When the test above counted the stream's bytes alone, it failed once in ten runs under
/// `taskpolicy -c background` ("paused at 262143 bytes"); with this fire after 50 renames it
/// failed every time.
#[test]
fn a_watchdog_fire_while_the_window_fills_counts_the_limits_view_it_resends() {
    let scratch = Scratch::new("cap-watchdog", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let _sent = fill_until_paused(&host, &bridge, &messages, thread, Some(50));
    // The view at open and the one the fire resent.
    assert!(
        messages.limits_received() >= 2,
        "{}",
        messages.limits_received()
    );
}

#[test]
fn a_refused_folder_is_rejected() {
    let scratch = Scratch::new("refused-folder", &["refused", "work"]);
    let refused = scratch.path().join("refused");
    let host = scratch.start(vec![refused.clone()]);
    let bridge = host.bridge();

    let home = scratch.path();
    let picked = block_on(bridge.pick_folder(&Picks(Some(refused)), Some(home))).unwrap();
    let folder = picked.unwrap().folder;
    let create = |folder| {
        envelope(HostCommand::CreateMatter {
            name: Title::new("Smith v Jones").unwrap(),
            source: MatterSource::ExistingFolder { folder },
            client: None,
            next_date: None,
        })
    };
    let refusal = block_on(bridge.dispatch(MAIN, create(folder))).unwrap_err();
    assert_eq!(
        refusal.code(),
        ErrorCode::MatterFolderIsSystem,
        "{refusal:?}"
    );

    // The data folder is always refused.
    let data = block_on(bridge.pick_folder(&Picks(Some(scratch.path().to_owned())), None));
    let refusal =
        block_on(bridge.dispatch(MAIN, create(data.unwrap().unwrap().folder))).unwrap_err();
    // It also holds the folder refused above, which the list names first.
    assert_eq!(
        refusal.code(),
        ErrorCode::MatterFolderIsSystem,
        "{refusal:?}"
    );

    // A folder outside the data folder works (one inside it is refused, as the matters root
    // is); a cancelled pick is `None`. The other scratch folder is outside this data folder.
    let other = Scratch::folder("refused-folder-other", &["work"]);
    let work = other.path().join("work");
    let picked = block_on(bridge.pick_folder(&Picks(Some(work)), Some(other.path()))).unwrap();
    let picked = picked.unwrap();
    assert_eq!(picked.display, "work");
    block_on(bridge.dispatch(MAIN, create(picked.folder))).unwrap();
    let cancelled = block_on(bridge.pick_folder(&Picks(None), Some(home))).unwrap();
    assert_eq!(cancelled, None);
}

#[test]
fn dispatch_refuses_a_folder_ref_it_did_not_issue_and_a_permission_answer() {
    let scratch = Scratch::new("dispatch-refusals", &["work"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();

    // Registered with the host, but not through this bridge's picker.
    let folder = block_on(host.host.register_folder(scratch.path().join("work"))).unwrap();
    let create = envelope(HostCommand::CreateMatter {
        name: Title::new("Smith v Jones").unwrap(),
        source: MatterSource::ExistingFolder { folder },
        client: None,
        next_date: None,
    });
    let refusal = block_on(bridge.dispatch(MAIN, create)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");

    let thread = host.thread();
    let answer = envelope(HostCommand::AnswerPermission {
        thread,
        request: PermissionRequestId::new(Uuid::now_v7()).unwrap(),
        option_id: AgentChoiceId::new("allow").unwrap(),
    });
    let refusal = block_on(bridge.dispatch(MAIN, answer)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
}

#[test]
fn dispatch_refuses_set_mode_and_set_config_option() {
    let scratch = Scratch::new("dispatch-mode", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let bypass = AgentChoiceId::new("bypassPermissions").unwrap();
    let commands = [
        HostCommand::SetMode {
            thread,
            mode_id: bypass.clone(),
        },
        HostCommand::SetConfigOption {
            thread,
            option_id: AgentChoiceId::new("mode").unwrap(),
            value: ConfigValue::Select(bypass),
        },
    ];
    for command in commands {
        let refusal = block_on(bridge.dispatch(MAIN, envelope(command.clone()))).unwrap_err();
        assert_eq!(
            refusal.code(),
            ErrorCode::CommandInvalid,
            "{command:?}: {refusal:?}"
        );
    }
}

/// A restart the host refuses (here a stopped host) tells the window with a `stream_failed`
/// message and leaves the stream followed, its retry scheduled; a `subscribe` the host refuses
/// returns the error and follows nothing.
#[test]
fn a_failed_restart_tells_the_window_and_waits_for_a_retry() {
    let scratch = Scratch::new("failed-restart", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let window = bridge.window(MAIN).unwrap();
    let other = StreamId::Matter(host.scratch());
    block_on(host.host.shutdown()).unwrap();

    let refusal = block_on(bridge.subscribe(MAIN, other, None)).unwrap_err();
    assert_ne!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    assert!(!lock(&window).state().follows(other));

    let restart = pump::follow(&window, stream(thread), None);
    pump::spawn_restarts(&host.host, &window, vec![restart]);
    let text = messages.next_text();
    let message: serde_json::Value = serde_json::from_str(&text).unwrap();
    let failed = &message["stream_failed"];
    assert_eq!(
        failed["stream"],
        serde_json::json!(stream(thread)),
        "{text}"
    );
    assert!(failed["error"]["code"].is_string(), "{text}");
    assert!(lock(&window).state().retry_at(stream(thread)).is_some());
    // The message counts: an ack of it is not more than was sent.
    block_on(bridge.ack(MAIN, Vec::new(), 1)).unwrap();
}

#[test]
fn stream_commands_need_open_stream_and_acks_are_bounded() {
    let scratch = Scratch::new("stream-commands", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let stream = StreamId::Matter(host.scratch());
    let refusal = block_on(bridge.subscribe(MAIN, stream, None)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid);

    let messages = open(&bridge, MAIN);
    block_on(bridge.subscribe(MAIN, stream, None)).unwrap();
    let reset = batch(messages.next_event());
    let cursor = reset.patches[0].cursor;
    let position = Ack {
        stream,
        epoch: reset.epoch,
        cursor,
    };
    // Sent: the limits view at open, and the Reset.
    let sent = 1 + messages.limits_received();
    let refusal = block_on(bridge.ack(MAIN, vec![position], sent + 1)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid);
    let refusal = block_on(bridge.ack(MAIN, vec![position, position], 1)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid);
    // A messages-only ack, then one with the position.
    block_on(bridge.ack(MAIN, Vec::new(), 1)).unwrap();
    block_on(bridge.ack(MAIN, vec![position], 1)).unwrap();
    bridge.unsubscribe(MAIN, stream).unwrap();
    assert_eq!(messages.drain(), Vec::<String>::new());
}

mod admit;
mod app;
mod connectors;
mod limits;
mod permission;
