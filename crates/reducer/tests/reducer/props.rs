//! Property tests over generated thread logs: the fold is deterministic, a snapshot plus the
//! tail equals a replay, deltas never change the visible text, and the changes rebuild the view.
//!
//! The generator emits only valid thread logs: `ThreadCreated`, then turns of a prompt, its
//! start, text chunks on up to three messages (one of thoughts, two of answers) with non-ASCII
//! text and chars JSON escapes, tool calls and their updates, permissions asked and answered,
//! files, and the turn's end. Each text chunk may also come as deltas first, split anywhere,
//! and a host may hold text in its durability batch and run its pending head ahead of the state
//! (commits in flight), under the host contract.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use clauseo_protocol::{
    AppendTarget, ContentBlock, DomainEvent, EntryId, Envelope, MessageId, PermissionRequestId,
    Seq, StreamHeader, TextKind, ToolCallId, ToolCallStatus, ViewEntry, ViewWindow, utf16_len,
};
use clauseo_reducer::{Anomaly, Applied, Snapshot, StreamState, ViewChange, apply, view_snapshot};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use crate::fixtures::{
    Log, files_changed, permission_requested, permission_resolved, start_turn, text,
    thread_created, tool_diff, tool_started, tool_status, turn_ended,
};
use crate::helpers::{self, delta, delta_at, empty, fold};

/// Text a chunk is made of: ASCII, 2-, 3- and 4-byte chars, and chars JSON escapes.
#[cfg(test)]
const PIECES: &[&str] = &[
    "Hello",
    " wörld",
    "😀",
    "日本語",
    "\"quoted\"",
    "back\\slash",
    "\n",
    "\t",
    "\u{1}",
    "é",
    " ",
];

/// One step of a generated turn.
#[cfg(test)]
#[derive(Debug, Clone)]
enum Item {
    /// A chunk on message `slot` (0 thoughts, 1 and 2 answers), maybe as deltas first, split
    /// at the char positions `cuts` (modulo the chunk's length).
    Text {
        slot: usize,
        text: String,
        cuts: Vec<usize>,
        deltas: bool,
    },
    /// A new tool call.
    Tool,
    /// An update of an earlier tool call: its status, or its content.
    Update { pick: usize, status: bool },
    /// A permission request, pending until a `Resolve` or the turn's end.
    Ask,
    /// The oldest pending request answered.
    Resolve,
    /// The turn's files.
    Files,
}

#[cfg(test)]
fn chunk() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 1..4).prop_map(|pieces| pieces.concat())
}

#[cfg(test)]
fn item() -> impl Strategy<Value = Item> {
    let text = (
        0..3usize,
        chunk(),
        prop::collection::vec(0..16usize, 0..3),
        any::<bool>(),
    )
        .prop_map(|(slot, text, cuts, deltas)| Item::Text {
            slot,
            text,
            cuts,
            deltas,
        });
    let update =
        (0..8usize, any::<bool>()).prop_map(|(pick, status)| Item::Update { pick, status });
    prop_oneof![
        5 => text,
        1 => Just(Item::Tool),
        1 => update,
        1 => Just(Item::Ask),
        1 => Just(Item::Resolve),
        1 => Just(Item::Files),
    ]
}

/// One to three turns of up to ten items.
#[cfg(test)]
fn script() -> impl Strategy<Value = Vec<Vec<Item>>> {
    prop::collection::vec(prop::collection::vec(item(), 0..10), 1..4)
}

/// `text` split at the char positions `cuts` into non-empty pieces.
#[cfg(test)]
fn split(text: &str, cuts: &[usize]) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut at: Vec<usize> = cuts.iter().map(|cut| cut % (chars.len() + 1)).collect();
    at.extend([0, chars.len()]);
    at.sort_unstable();
    at.dedup();
    at.windows(2)
        .map(|pair| chars[pair[0]..pair[1]].iter().collect())
        .collect()
}

/// The log of `script`, each envelope with the delta pieces the host shows before it.
#[cfg(test)]
fn build(script: &[Vec<Item>]) -> Vec<(Envelope, Vec<String>)> {
    let mut log = Log::thread(10).unwrap();
    let mut deltas = vec![Vec::new()];
    log.push(thread_created().unwrap()).unwrap();
    for (number, items) in script.iter().enumerate() {
        let turn = start_turn(&mut log, &format!("Prompt {number}: «review» 😀")).unwrap();
        deltas.extend([Vec::new(), Vec::new()]);
        let mut messages: [Option<MessageId>; 3] = [None; 3];
        let mut tools: Vec<ToolCallId> = Vec::new();
        let mut pending: VecDeque<PermissionRequestId> = VecDeque::new();
        for item in items {
            let (event, pieces): (DomainEvent, Vec<String>) = match item {
                Item::Text {
                    slot,
                    text: chunk,
                    cuts,
                    deltas,
                } => {
                    let message = *messages[*slot].get_or_insert_with(|| log.fresh().unwrap());
                    let kind = if *slot == 0 {
                        TextKind::Thought
                    } else {
                        TextKind::Answer
                    };
                    let pieces = if *deltas { split(chunk, cuts) } else { vec![] };
                    (text(turn, message, kind, chunk.clone()), pieces)
                }
                Item::Tool => {
                    let tool_call = log.fresh().unwrap();
                    tools.push(tool_call);
                    (tool_started(turn, tool_call, tools.len()).unwrap(), vec![])
                }
                Item::Update { pick, status } => {
                    let Some(&tool_call) = tools.get(pick % tools.len().max(1)) else {
                        continue;
                    };
                    let event = if *status {
                        tool_status(tool_call, ToolCallStatus::Completed)
                    } else {
                        tool_diff(tool_call, *pick).unwrap()
                    };
                    (event, vec![])
                }
                Item::Ask => {
                    let request = log.fresh().unwrap();
                    pending.push_back(request);
                    (permission_requested(request, tools.last().copied()), vec![])
                }
                Item::Resolve => {
                    let Some(request) = pending.pop_front() else {
                        continue;
                    };
                    (permission_resolved(request), vec![])
                }
                Item::Files => (files_changed(turn).unwrap(), vec![]),
            };
            log.push(event).unwrap();
            deltas.push(pieces);
        }
        for request in pending {
            log.push(permission_resolved(request)).unwrap();
            deltas.push(Vec::new());
        }
        let checkpoint_after = log.fresh().unwrap();
        log.push(turn_ended(turn, checkpoint_after).unwrap())
            .unwrap();
        deltas.push(Vec::new());
    }
    log.into_events().into_iter().zip(deltas).collect()
}

#[cfg(test)]
fn events_of(stream: &[(Envelope, Vec<String>)]) -> Vec<Envelope> {
    stream
        .iter()
        .map(|(envelope, _)| envelope.clone())
        .collect()
}

/// Applies the deltas of `envelope`'s text, as the host would show them before it.
#[cfg(test)]
fn apply_deltas(state: &mut StreamState, envelope: &Envelope, pieces: &[String]) -> Vec<Applied> {
    let DomainEvent::TextAppended(event) = &envelope.event else {
        return Vec::new();
    };
    pieces
        .iter()
        .map(|piece| delta(state, event.turn, event.message, event.kind, piece))
        .collect()
}

/// A webview's model: entries by id, fed only changes.
#[cfg(test)]
#[derive(Debug, Default)]
struct Model(BTreeMap<EntryId, ViewEntry>);

#[cfg(test)]
impl Model {
    fn feed(&mut self, changes: &[ViewChange]) -> Result<(), TestCaseError> {
        for change in changes {
            match change {
                ViewChange::Upsert(entry) => {
                    self.0.insert(entry.id(), (**entry).clone());
                }
                ViewChange::Remove(id) => {
                    self.0.remove(id);
                }
                ViewChange::AppendText {
                    message_id,
                    target,
                    from_len,
                    text,
                } => {
                    let shown = self.text_mut(*message_id, *target)?;
                    prop_assert_eq!(utf16_len(shown), *from_len as usize, "from_len");
                    shown.push_str(text);
                }
            }
        }
        Ok(())
    }

    fn text_mut(
        &mut self,
        message: MessageId,
        target: AppendTarget,
    ) -> Result<&mut String, TestCaseError> {
        let found = match target {
            AppendTarget::AgentText => match self.0.get_mut(&EntryId::AgentText(message)) {
                Some(ViewEntry::AgentText(entry)) => Some(&mut entry.text),
                _ => None,
            },
            AppendTarget::UserBlock(index) => {
                match self.0.get_mut(&EntryId::UserMessage(message)) {
                    Some(ViewEntry::UserMessage(entry)) => {
                        match entry.content.get_mut(index as usize) {
                            Some(ContentBlock::Text { text }) => Some(text),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }
        };
        found.ok_or_else(|| TestCaseError::fail(format!("no text for {message} {target:?}")))
    }

    /// The model as a view window: the header and every other entry by order.
    fn window(&self) -> ViewWindow {
        let mut header = None;
        let mut entries = Vec::new();
        for entry in self.0.values() {
            match entry {
                ViewEntry::Header(h) => header = Some(StreamHeader::Thread(h.clone())),
                other => entries.push(other.clone()),
            }
        }
        entries.sort_by_key(ViewEntry::order);
        ViewWindow {
            header,
            entries,
            has_older: false,
        }
    }
}

#[cfg(test)]
fn snapshot_bytes(state: &StreamState) -> Vec<u8> {
    Snapshot::of(state).to_bytes().unwrap()
}

#[cfg(test)]
fn is_delta_anomaly(anomaly: &Anomaly) -> bool {
    matches!(
        anomaly,
        Anomaly::DeltaMismatch { .. } | Anomaly::OrderCorrected { .. }
    )
}

/// What [`play_ahead`] leaves: the state with deltas, the fold of the same events without
/// them, every change in the order it was emitted, and how many first deltas closed a batch
/// that held another message's unsent text.
#[cfg(test)]
struct Ahead {
    state: StreamState,
    plain: StreamState,
    applied: Vec<Applied>,
    handoffs: usize,
}

/// The sending side of [`play_ahead`]: events sent and not yet applied, the pending head, and
/// the state and plain fold that apply them.
#[cfg(test)]
struct Sender<'a, F> {
    lags: &'a [usize],
    flight: VecDeque<&'a Envelope>,
    head: Option<Seq>,
    sent: usize,
    ahead: Ahead,
    each: F,
}

#[cfg(test)]
impl<'a, F: FnMut(&StreamState, &StreamState) -> Result<(), TestCaseError>> Sender<'a, F> {
    /// Sends `envelope`; the state then applies up to `lags[n % len]` of the sent events, `n`
    /// counting the sends.
    fn send(&mut self, envelope: &'a Envelope) -> Result<(), TestCaseError> {
        self.flight.push_back(envelope);
        self.head = Some(envelope.seq);
        let lag = self.lags[self.sent % self.lags.len()];
        self.sent += 1;
        self.catch_up(lag)
    }

    /// Sends the batch's events, oldest first.
    fn flush(&mut self, batch: &mut Vec<&'a Envelope>) -> Result<(), TestCaseError> {
        for envelope in batch.drain(..) {
            self.send(envelope)?;
        }
        Ok(())
    }

    /// Applies sent events until at most `lag` are in flight.
    fn catch_up(&mut self, lag: usize) -> Result<(), TestCaseError> {
        while self.flight.len() > lag {
            let Some(sent) = self.flight.pop_front() else {
                break;
            };
            let ahead = &mut self.ahead;
            ahead.applied.push(apply(&mut ahead.state, sent).unwrap());
            apply(&mut ahead.plain, sent).unwrap();
            (self.each)(&ahead.state, &ahead.plain)?;
        }
        Ok(())
    }
}

/// Plays `stream` as a host with a durability batch and commits in flight, under the host
/// contract (`lib.rs`). Each text event's text is shown first as deltas (the whole text when it
/// has no pieces) and waits in the batch. The batch is sent before any non-text event, before a
/// message's first delta, whose `pending_seq` is then the head that results, and after a text
/// event when `flushes[i % len]` says the batch timer fired. So a new message's first delta often
/// meets another message's unsent tail, as an answer meets its thought's. After each send the
/// state applies up to `lags[n % len]` of the sent events, so its `seq` trails the pending
/// head. `each` runs after every step with the state and the plain fold.
#[cfg(test)]
fn play_ahead(
    stream: &[(Envelope, Vec<String>)],
    lags: &[usize],
    flushes: &[bool],
    each: impl FnMut(&StreamState, &StreamState) -> Result<(), TestCaseError>,
) -> Result<Ahead, TestCaseError> {
    let events = events_of(stream);
    let mut sender = Sender {
        lags,
        flight: VecDeque::new(),
        head: None,
        sent: 0,
        ahead: Ahead {
            state: empty(&events),
            plain: empty(&events),
            applied: Vec::new(),
            handoffs: 0,
        },
        each,
    };
    let mut batch: Vec<&Envelope> = Vec::new();
    let mut shown: BTreeSet<MessageId> = BTreeSet::new();
    for (index, (envelope, pieces)) in stream.iter().enumerate() {
        let DomainEvent::TextAppended(event) = &envelope.event else {
            sender.flush(&mut batch)?;
            sender.send(envelope)?;
            continue;
        };
        if shown.insert(event.message) {
            // The message's first delta closes the batch.
            if batch
                .iter()
                .any(|held| is_text_of_other(held, event.message))
            {
                sender.ahead.handoffs += 1;
            }
            sender.flush(&mut batch)?;
        }
        let whole = [event.text.clone()];
        let pieces = if pieces.is_empty() {
            &whole[..]
        } else {
            pieces
        };
        let pending = sender.head.unwrap();
        for piece in pieces {
            let (turn, message, kind) = (event.turn, event.message, event.kind);
            let ahead = &mut sender.ahead;
            let applied = delta_at(&mut ahead.state, turn, message, kind, piece, pending);
            ahead.applied.push(applied);
        }
        (sender.each)(&sender.ahead.state, &sender.ahead.plain)?;
        batch.push(envelope);
        if flushes[index % flushes.len()] {
            sender.flush(&mut batch)?;
        }
    }
    sender.flush(&mut batch)?;
    sender.catch_up(0)?;
    Ok(sender.ahead)
}

/// Whether `envelope` is text of a message other than `message`.
#[cfg(test)]
fn is_text_of_other(envelope: &Envelope, message: MessageId) -> bool {
    matches!(&envelope.event, DomainEvent::TextAppended(event) if event.message != message)
}

#[cfg(test)]
fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 128,
        rng_seed: RngSeed::Fixed(0x5eed_0006),
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn fold_is_deterministic(script in script()) {
        let events = events_of(&build(&script));
        let first = fold(&events);
        let second = fold(&events);
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(snapshot_bytes(&first), snapshot_bytes(&second));
    }

    #[test]
    fn snapshot_then_tail_equals_replay(script in script(), cut in any::<prop::sample::Index>()) {
        let events = events_of(&build(&script));
        let n = cut.index(events.len() + 1);
        let mut head = empty(&events);
        crate::helpers::apply_all(&mut head, &events[..n]);
        let mut state = crate::helpers::round_trip(&head);
        crate::helpers::apply_all(&mut state, &events[n..]);
        prop_assert_eq!(state, fold(&events));
    }

    #[test]
    fn coalescing_never_changes_visible_text(script in script()) {
        let stream = build(&script);
        let events = events_of(&stream);
        let plain = fold(&events);
        let mut state = empty(&events);
        for (envelope, pieces) in &stream {
            for applied in apply_deltas(&mut state, envelope, pieces) {
                prop_assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
            }
            let applied = apply(&mut state, envelope).unwrap();
            let bad: Vec<_> = applied.anomalies.iter().filter(|a| is_delta_anomaly(a)).collect();
            prop_assert!(bad.is_empty(), "{:?}", bad);
        }
        prop_assert_eq!(view_snapshot(&state), view_snapshot(&plain));
        // With every delta confirmed, the snapshot is the log's.
        prop_assert_eq!(snapshot_bytes(&state), snapshot_bytes(&plain));
    }

    #[test]
    fn patches_rebuild_view_snapshot(script in script()) {
        let stream = build(&script);
        let events = events_of(&stream);
        let mut state = empty(&events);
        let mut model = Model::default();
        for (envelope, pieces) in &stream {
            for applied in apply_deltas(&mut state, envelope, pieces) {
                model.feed(&applied.changes)?;
            }
            model.feed(&apply(&mut state, envelope).unwrap().changes)?;
        }
        let window = view_snapshot(&state);
        prop_assert!(!window.has_older);
        prop_assert_eq!(model.window(), window);
    }

    #[test]
    fn pending_head_ahead_keeps_order_and_view(
        script in script(),
        lags in prop::collection::vec(0..3usize, 1..8),
        flushes in prop::collection::vec(prop::bool::weighted(0.25), 1..8),
    ) {
        let stream = build(&script);
        let ahead = play_ahead(&stream, &lags, &flushes, |_, _| Ok(()))?;
        let mut model = Model::default();
        for applied in &ahead.applied {
            prop_assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
            model.feed(&applied.changes)?;
        }
        let window = view_snapshot(&ahead.state);
        prop_assert_eq!(&window, &view_snapshot(&ahead.plain));
        prop_assert_eq!(model.window(), window);
        prop_assert_eq!(snapshot_bytes(&ahead.state), snapshot_bytes(&ahead.plain));
    }

    #[test]
    fn snapshot_with_unconfirmed_deltas_is_the_fold_so_far(
        script in script(),
        lags in prop::collection::vec(0..3usize, 1..8),
        flushes in prop::collection::vec(prop::bool::weighted(0.25), 1..8),
    ) {
        let stream = build(&script);
        play_ahead(&stream, &lags, &flushes, |state, plain| {
            prop_assert_eq!(&helpers::round_trip(state), plain);
            Ok(())
        })?;
    }
}

#[test]
fn play_ahead_meets_unsent_tails() {
    // A thought in two chunks, held in the batch, then an answer and a second answer: each
    // answer's first delta meets the unsent text before it. No anomaly, and the view is the log's.
    let item = |slot: usize, text: &str| Item::Text {
        slot,
        text: text.to_owned(),
        cuts: vec![3],
        deltas: true,
    };
    let script = vec![vec![
        item(0, "Reading é the lease."),
        item(0, " Clause 4 😀."),
        item(1, "The break clause"),
        item(1, " needs notice."),
        item(2, "Draft: «six months»."),
    ]];
    let stream = build(&script);
    for lags in [[0], [1], [2]] {
        let ahead = play_ahead(&stream, &lags, &[false], |_, _| Ok(())).unwrap();
        assert_eq!(ahead.handoffs, 2);
        for applied in &ahead.applied {
            assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
        }
        assert_eq!(view_snapshot(&ahead.state), view_snapshot(&ahead.plain));
        assert_eq!(snapshot_bytes(&ahead.state), snapshot_bytes(&ahead.plain));
    }
}
