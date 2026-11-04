//! What the index takes from committed events: a fold per stream, from envelopes to writes.
//!
//! A thread's [`ThreadMemo`] is the fold of its events up to the stream's watermark: the
//! prompts' and tool calls' entry orders (for `QueuedPromptEdited` and a renamed tool call),
//! each turn's entries (for `StepUndone`) and the text of the answers of turns still running.
//! An answer is written once, whole, when its turn ends (`TurnEnded`, `TurnFailed` or
//! `TurnStoppedAtLimit`), assembled as the reducer assembles an agent message: its
//! `TextAppended` chunks joined in seq order, its kind the first chunk's, its order the first
//! chunk's seq. Thoughts are not indexed.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    CheckpointId, ContentBlock, DomainEvent, Envelope, MatterId, MessageId, Seq, SteerOutcome,
    SteerResolved, StopReason, TextAppended, TextKind, ThreadId, Timestamp, ToolCallId, TurnId,
};
use clauseo_search::EntryKind;

/// One change to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Write {
    Entry {
        matter: MatterId,
        thread: ThreadId,
        order: Seq,
        kind: EntryKind,
        at: Timestamp,
        text: String,
    },
    ThreadTitle {
        matter: MatterId,
        thread: ThreadId,
        at: Timestamp,
        title: String,
    },
    MatterTitle {
        matter: MatterId,
        at: Timestamp,
        title: String,
    },
    Archived {
        matter: MatterId,
        thread: ThreadId,
    },
    /// An undone step's entries, or a withdrawn queued prompt's.
    Undone {
        thread: ThreadId,
        entries: Vec<Seq>,
    },
    /// The matter's file names as of `checkpoint`.
    Files {
        matter: MatterId,
        at: Timestamp,
        checkpoint: CheckpointId,
    },
}

/// An agent message whose turn has not ended.
#[derive(Debug, Clone)]
struct Open {
    turn: TurnId,
    order: Seq,
    at: Timestamp,
    /// Kind answer; a thought keeps no text.
    answer: bool,
    text: String,
}

/// The fold of one thread's events so far.
#[derive(Debug, Clone)]
pub(super) struct ThreadMemo {
    thread: ThreadId,
    matter: Option<MatterId>,
    prompts: BTreeMap<MessageId, Seq>,
    tools: BTreeMap<ToolCallId, Seq>,
    open: BTreeMap<MessageId, Open>,
    turns: BTreeMap<TurnId, Vec<Seq>>,
    /// Each prompt's turn, as the reducer last set it (a steer moves it).
    prompt_turns: BTreeMap<MessageId, TurnId>,
    /// Turns that started.
    started: BTreeSet<TurnId>,
}

impl ThreadMemo {
    pub(super) const fn new(thread: ThreadId) -> Self {
        Self {
            thread,
            matter: None,
            prompts: BTreeMap::new(),
            tools: BTreeMap::new(),
            open: BTreeMap::new(),
            turns: BTreeMap::new(),
            prompt_turns: BTreeMap::new(),
            started: BTreeSet::new(),
        }
    }

    /// The matter, once `ThreadCreated` named it.
    pub(super) const fn matter(&self) -> Option<MatterId> {
        self.matter
    }

    /// Folds one envelope of this thread, pushing the writes it makes.
    pub(super) fn feed(&mut self, envelope: &Envelope, out: &mut Vec<Write>) {
        let (seq, at, thread) = (envelope.seq, envelope.ts, self.thread);
        let event = &envelope.event;
        if let DomainEvent::ThreadCreated(event) = event {
            self.matter = Some(event.matter);
            if let Some(title) = &event.title {
                out.push(Write::ThreadTitle {
                    matter: event.matter,
                    thread,
                    at,
                    title: title.to_string(),
                });
            }
        } else if let DomainEvent::ThreadRenamed(event) = event {
            if let Some(matter) = self.matter {
                out.push(Write::ThreadTitle {
                    matter,
                    thread,
                    at,
                    title: event.title.to_string(),
                });
            }
        } else if let DomainEvent::ThreadArchived(_) = event {
            if let Some(matter) = self.matter {
                out.push(Write::Archived { matter, thread });
            }
        } else if let DomainEvent::UserMessage(event) = event {
            self.prompts.insert(event.message, seq);
            self.prompt_turns.insert(event.message, event.turn);
            self.turns.entry(event.turn).or_default().push(seq);
            self.entry(out, seq, EntryKind::Prompt, at, prompt_text(&event.content));
        } else if let DomainEvent::QueuedPromptEdited(event) = event {
            if let Some(&order) = self.prompts.get(&event.message) {
                self.entry(
                    out,
                    order,
                    EntryKind::Prompt,
                    at,
                    prompt_text(&event.content),
                );
            }
        } else if let DomainEvent::TextAppended(event) = event {
            self.text(seq, at, event);
        } else if let DomainEvent::ToolCallStarted(event) = event {
            self.tools.insert(event.tool_call, seq);
            self.turns.entry(event.turn).or_default().push(seq);
            self.entry(out, seq, EntryKind::ToolTitle, at, event.title.to_string());
        } else if let DomainEvent::ToolCallUpdated(event) = event {
            if let (Some(title), Some(&order)) =
                (&event.patch.title, self.tools.get(&event.tool_call))
            {
                self.entry(out, order, EntryKind::ToolTitle, at, title.to_string());
            }
        } else if let DomainEvent::SteerResolved(event) = event {
            self.steer_resolved(event);
        } else if let DomainEvent::TurnStarted(event) = event {
            self.started.insert(event.turn);
        } else if let Some(turn) = turn_end(event) {
            self.turn_ended(turn, out);
            let never_sent = matches!(event, DomainEvent::TurnEnded(ended)
                if matches!(ended.stop, StopReason::Cancelled | StopReason::NotAllowed));
            if never_sent && !self.started.contains(&turn) {
                self.withdrawn(turn, out);
            }
        } else if let DomainEvent::StepUndone(event) = event
            && let Some(entries) = self.turns.remove(&event.turn)
        {
            out.push(Write::Undone { thread, entries });
        }
    }

    /// A steer resolved: its prompt now answers the turn the outcome names, as the reducer sets
    /// the prompt's `turn`, so an undo of that turn takes it.
    fn steer_resolved(&mut self, event: &SteerResolved) {
        let turn = match &event.outcome {
            SteerOutcome::Injected { turn }
            | SteerOutcome::Requeued { turn }
            | SteerOutcome::SentNow { turn } => *turn,
            SteerOutcome::Unknown { .. } => return,
        };
        let (Some(&order), Some(old)) = (
            self.prompts.get(&event.message),
            self.prompt_turns.insert(event.message, turn),
        ) else {
            return;
        };
        if old == turn {
            return;
        }
        if let Some(entries) = self.turns.get_mut(&old) {
            entries.retain(|entry| *entry != order);
        }
        self.turns.entry(turn).or_default().push(order);
    }

    /// A turn that ended cancelled without starting was a queued prompt taken back
    /// (`RemoveQueuedPrompt`): it was never sent, so its prompt leaves the index.
    fn withdrawn(&mut self, turn: TurnId, out: &mut Vec<Write>) {
        if let Some(entries) = self.turns.remove(&turn)
            && !entries.is_empty()
        {
            out.push(Write::Undone {
                thread: self.thread,
                entries,
            });
        }
    }

    /// A chunk of an agent message: the message opens at its first chunk, with that chunk's
    /// kind; an answer keeps every chunk's text.
    fn text(&mut self, seq: Seq, at: Timestamp, event: &TextAppended) {
        if let Some(open) = self.open.get_mut(&event.message) {
            if open.answer {
                open.text.push_str(&event.text);
            }
            return;
        }
        let answer = event.kind == TextKind::Answer;
        if answer {
            self.turns.entry(event.turn).or_default().push(seq);
        }
        let text = if answer {
            event.text.clone()
        } else {
            String::new()
        };
        self.open.insert(
            event.message,
            Open {
                turn: event.turn,
                order: seq,
                at,
                answer,
                text,
            },
        );
    }

    fn entry(
        &self,
        out: &mut Vec<Write>,
        order: Seq,
        kind: EntryKind,
        at: Timestamp,
        text: String,
    ) {
        if let Some(matter) = self.matter {
            out.push(Write::Entry {
                matter,
                thread: self.thread,
                order,
                kind,
                at,
                text,
            });
        }
    }

    /// Writes the turn's answers, whole, in order.
    fn turn_ended(&mut self, turn: TurnId, out: &mut Vec<Write>) {
        let ended: Vec<MessageId> = self
            .open
            .iter()
            .filter(|(_, open)| open.turn == turn)
            .map(|(message, _)| *message)
            .collect();
        let mut answers: Vec<Open> = ended
            .iter()
            .filter_map(|message| self.open.remove(message))
            .filter(|open| open.answer)
            .collect();
        answers.sort_by_key(|open| open.order);
        for open in answers {
            self.entry(out, open.order, EntryKind::Answer, open.at, open.text);
        }
    }
}

/// The turn an event ends, for the three that end one.
const fn turn_end(event: &DomainEvent) -> Option<TurnId> {
    if let DomainEvent::TurnEnded(event) = event {
        Some(event.turn)
    } else if let DomainEvent::TurnFailed(event) = event {
        Some(event.turn)
    } else if let DomainEvent::TurnStoppedAtLimit(event) = event {
        Some(event.turn)
    } else {
        None
    }
}

/// Folds one envelope of matter `matter`'s stream.
pub(super) fn feed_matter(matter: MatterId, envelope: &Envelope, out: &mut Vec<Write>) {
    let at = envelope.ts;
    let event = &envelope.event;
    let checkpoint = if let DomainEvent::MatterCreated(event) = event {
        out.push(Write::MatterTitle {
            matter,
            at,
            title: event.name().to_owned(),
        });
        return;
    } else if let DomainEvent::CheckpointRecorded(event) = event {
        event.checkpoint()
    } else if let DomainEvent::OutsideEdits(event) = event {
        event.checkpoint
    } else if let DomainEvent::VersionRestored(event) = event {
        event.checkpoint
    } else {
        return;
    };
    out.push(Write::Files {
        matter,
        at,
        checkpoint,
    });
}

/// A prompt's text: its text blocks, one per line.
fn prompt_text(content: &[ContentBlock]) -> String {
    let mut text = String::new();
    for block in content {
        if let ContentBlock::Text { text: part, .. } = block {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
    }
    text
}
