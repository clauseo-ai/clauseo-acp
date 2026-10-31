//! What the tests read from the host: event subscriptions collected into envelopes, view
//! subscriptions collected into patches, and small lookups over both.

use std::fmt::Write as _;

use clauseo_host::{EncodedEvent, EventSubscription, Subscription};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, DetachReason, DomainEvent, Envelope, FilesChanged, HostEvent,
    PatchBatch, PermissionRequestId, Seq, StopReason, TextKind, TurnEnded, TurnId, TurnQueued,
    ViewPatch, ViewPatchBody,
};
use futures::{FutureExt, StreamExt};

use crate::support::{WATCHDOG, within};

/// The envelopes an event subscription delivered, in order, with the `CaughtUp`s between.
pub(crate) struct Events {
    sub: EventSubscription,
    /// Every envelope received, in arrival order.
    pub(crate) seen: Vec<Envelope>,
    /// For each envelope in `seen`, the index of the `HostEvent::Events` it came in.
    pub(crate) batch: Vec<usize>,
    /// Each `CaughtUp`: how many envelopes came before it, and its `last_seq`.
    pub(crate) caught_up: Vec<(usize, Seq)>,
    batches: usize,
    ended: bool,
}

impl Events {
    pub(crate) fn new(sub: EventSubscription) -> Self {
        Self {
            sub,
            seen: Vec::new(),
            batch: Vec::new(),
            caught_up: Vec::new(),
            batches: 0,
            ended: false,
        }
    }

    fn take(&mut self, item: Option<HostEvent>) {
        match item {
            Some(HostEvent::Events(envelopes)) => {
                for envelope in envelopes {
                    self.seen.push(envelope);
                    self.batch.push(self.batches);
                }
                self.batches += 1;
            }
            Some(HostEvent::CaughtUp { last_seq, .. }) => {
                self.caught_up.push((self.seen.len(), last_seq));
            }
            Some(other) => panic!("an event subscription delivered {other:?}"),
            None => self.ended = true,
        }
    }

    /// Reads until `done` holds over what was received; fails the test with what was seen
    /// when the stream ends first or the watchdog fires.
    pub(crate) fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) -> &mut Self {
        let finished = within(async {
            while !done(self) {
                if self.ended {
                    return false;
                }
                let item = self.sub.next().await;
                self.take(item);
            }
            true
        });
        match finished {
            Some(true) => self,
            Some(false) => panic!("stream ended before {what}; seen: {}", self.dump()),
            None => panic!("no {what} within {WATCHDOG:?}; seen: {}", self.dump()),
        }
    }

    /// Reads until the first `CaughtUp`.
    pub(crate) fn until_caught_up(&mut self) -> &mut Self {
        self.until("CaughtUp", |e| !e.caught_up.is_empty())
    }

    /// Reads until `turn`'s `TurnEnded` and returns it.
    pub(crate) fn until_turn_ended(&mut self, turn: TurnId) -> TurnEnded {
        self.until("TurnEnded", |e| e.turn_ended(turn).is_some());
        self.turn_ended(turn).unwrap()
    }

    /// Reads until the host ends the stream (it stopped).
    pub(crate) fn until_end(&mut self) -> &mut Self {
        self.until("the end of the stream", |e| e.ended)
    }

    /// Takes whatever is already in the channel, without waiting.
    pub(crate) fn drain_ready(&mut self) -> &mut Self {
        while !self.ended {
            match self.sub.next().now_or_never() {
                Some(item) => self.take(item),
                None => break,
            }
        }
        self
    }

    /// The events received.
    pub(crate) fn events(&self) -> impl Iterator<Item = &DomainEvent> {
        self.seen.iter().map(|envelope| &envelope.event)
    }

    /// The kinds received, in order.
    pub(crate) fn kinds(&self) -> Vec<&str> {
        self.events().map(DomainEvent::kind).collect()
    }

    /// One line per envelope, for failure messages.
    pub(crate) fn dump(&self) -> String {
        let mut out = String::new();
        for envelope in &self.seen {
            write!(out, "\n  #{} {:?}", envelope.seq.get(), envelope.event).unwrap();
        }
        if self.caught_up.is_empty() {
            out.push_str("\n  (no CaughtUp)");
        }
        out
    }

    /// The index in `seen` of the first event `find` accepts.
    pub(crate) fn position(&self, find: impl Fn(&DomainEvent) -> bool) -> Option<usize> {
        self.seen.iter().position(|envelope| find(&envelope.event))
    }

    /// How many events `find` accepts.
    pub(crate) fn count(&self, find: impl Fn(&DomainEvent) -> bool) -> usize {
        self.events().filter(|event| find(event)).count()
    }

    /// `turn`'s `TurnEnded`, when received.
    pub(crate) fn turn_ended(&self, turn: TurnId) -> Option<TurnEnded> {
        self.events().find_map(|event| match event {
            DomainEvent::TurnEnded(ended) if ended.turn == turn => Some(*ended),
            _ => None,
        })
    }

    /// The seq of the last envelope received.
    pub(crate) fn head(&self) -> u64 {
        self.seen.last().map_or(0, |envelope| envelope.seq.get())
    }

    /// The checkpoint of the first `CheckpointRecorded` of `kind` (a matter stream).
    pub(crate) fn checkpoint(&self, kind: CheckpointKind) -> Option<CheckpointId> {
        self.events().find_map(|event| match event {
            DomainEvent::CheckpointRecorded(recorded) if recorded.kind() == kind => {
                Some(recorded.checkpoint())
            }
            _ => None,
        })
    }

    /// The `PermissionRequested` ids, in order.
    pub(crate) fn permission_requests(&self) -> Vec<PermissionRequestId> {
        self.events()
            .filter_map(|event| match event {
                DomainEvent::PermissionRequested(requested) => Some(requested.request),
                _ => None,
            })
            .collect()
    }

    /// `turn`'s `FilesChanged`, when received.
    pub(crate) fn files_changed(&self, turn: TurnId) -> Option<FilesChanged> {
        self.events().find_map(|event| match event {
            DomainEvent::FilesChanged(changed) if changed.turn == turn => Some(changed.clone()),
            _ => None,
        })
    }

    /// The last `TurnQueued` of `turn`, when received.
    pub(crate) fn last_queued(&self, turn: TurnId) -> Option<TurnQueued> {
        self.events()
            .filter_map(|event| match event {
                DomainEvent::TurnQueued(queued) if queued.turn() == turn => Some(*queued),
                _ => None,
            })
            .last()
    }

    /// The text of every `TextAppended` of `turn`, joined in order.
    pub(crate) fn turn_text(&self, turn: TurnId) -> String {
        self.events()
            .filter_map(|event| match event {
                DomainEvent::TextAppended(appended) if appended.turn == turn => {
                    Some(appended.text.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// The text of every `TextAppended` of `kind`, joined in order.
    pub(crate) fn text(&self, kind: TextKind) -> String {
        self.events()
            .filter_map(|event| match event {
                DomainEvent::TextAppended(appended) if appended.kind == kind => {
                    Some(appended.text.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// Fails the test, with the agent's stderr tail, when the agent exited with code 3: a
    /// scenario mismatch, which means the host sent something the scenario did not expect.
    pub(crate) fn assert_no_mismatch(&self) {
        for event in self.events() {
            if let DomainEvent::AgentSessionDetached(detached) = event
                && let DetachReason::Exited {
                    code: Some(3),
                    stderr_tail,
                } = &detached.reason
            {
                panic!("fake_agent reported a mismatch (exit 3):\n{stderr_tail}");
            }
        }
    }
}

/// Whether `event` is `turn`'s `TurnEnded` with `stop`.
pub(crate) fn ended_with(event: &DomainEvent, turn: TurnId, stop: StopReason) -> bool {
    matches!(event, DomainEvent::TurnEnded(ended) if ended.turn == turn && ended.stop == stop)
}

/// Whether `event` is a `TurnStarted` of `turn`.
pub(crate) fn started(event: &DomainEvent, turn: TurnId) -> bool {
    matches!(event, DomainEvent::TurnStarted(started) if started.turn == turn)
}

/// The patches a view subscription delivered, each with the encoded event it came in.
pub(crate) struct View {
    sub: Subscription,
    /// Every encoded event received, in order.
    pub(crate) items: Vec<EncodedEvent>,
    /// Every batch, decoded from `items`.
    pub(crate) batches: Vec<PatchBatch>,
}

impl View {
    pub(crate) fn new(sub: Subscription) -> Self {
        Self {
            sub,
            items: Vec::new(),
            batches: Vec::new(),
        }
    }

    fn take(&mut self, item: EncodedEvent) {
        match item.decode() {
            Ok(HostEvent::Patches(batch)) => self.batches.push(batch),
            Ok(other) => panic!("a view subscription delivered {other:?}"),
            Err(error) => panic!("a view event does not decode: {error}: {}", item.json()),
        }
        self.items.push(item);
    }

    /// Reads until `done` holds; fails the test when the stream ends or the watchdog fires.
    pub(crate) fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) -> &mut Self {
        let finished = within(async {
            while !done(self) {
                match self.sub.next().await {
                    Some(item) => self.take(item),
                    None => return false,
                }
            }
            true
        });
        match finished {
            Some(true) => self,
            Some(false) => panic!("view ended before {what}; seen: {}", self.dump()),
            None => panic!("no {what} within {WATCHDOG:?}; seen: {}", self.dump()),
        }
    }

    /// Reads until at least `n` encoded events have come.
    pub(crate) fn until_items(&mut self, n: usize) -> &mut Self {
        self.until(&format!("{n} view events"), |v| v.items.len() >= n)
    }

    /// Takes whatever is already in the channel, without waiting.
    pub(crate) fn drain_ready(&mut self) -> &mut Self {
        while let Some(Some(item)) = self.sub.next().now_or_never() {
            self.take(item);
        }
        self
    }

    /// Every patch received, in order.
    pub(crate) fn patches(&self) -> impl Iterator<Item = &ViewPatch> {
        self.batches.iter().flat_map(|batch| batch.patches.iter())
    }

    /// The cursor of the last patch received.
    pub(crate) fn cursor(&self) -> Option<u64> {
        self.patches().last().map(|patch| patch.cursor.get())
    }

    /// Whether any patch received holds `needle` in its JSON.
    pub(crate) fn mentions(&self, needle: &str) -> bool {
        self.items.iter().any(|item| item.json().contains(needle))
    }

    /// Whether the first patch received is a `Reset`.
    pub(crate) fn starts_with_reset(&self) -> bool {
        matches!(
            self.patches().next().map(|patch| &patch.body),
            Some(ViewPatchBody::Reset { .. })
        )
    }

    /// How many `Reset`s were received.
    pub(crate) fn resets(&self) -> usize {
        self.patches()
            .filter(|patch| matches!(patch.body, ViewPatchBody::Reset { .. }))
            .count()
    }

    /// One line per patch, for failure messages.
    pub(crate) fn dump(&self) -> String {
        let mut out = String::new();
        for patch in self.patches() {
            let json = serde_json::to_string(&patch.body).unwrap_or_default();
            let short: String = json.chars().take(160).collect();
            write!(out, "\n  @{} {short}", patch.cursor.get()).unwrap();
        }
        out
    }
}
