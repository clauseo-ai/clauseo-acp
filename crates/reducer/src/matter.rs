//! The fold of a matter's stream: its header and its Versions timeline.

use clauseo_protocol::{
    DomainEvent, EntryId, Envelope, MarkerEntry, MarkerKind, MatterHeaderEntry, VersionEntry,
    ViewEntry,
};

use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::MatterState;

/// Folds one envelope of the matter's stream; the caller has checked its stream and `seq`.
pub(crate) fn fold(state: &mut MatterState, envelope: &Envelope, out: &mut Applied) {
    let seq = envelope.seq;
    match &envelope.event {
        DomainEvent::MatterCreated(event) => {
            if state.header.is_some() {
                out.anomalies.push(Anomaly::Duplicate {
                    event: "matter_created",
                    target: EntryId::MatterHeader(state.matter),
                });
                return;
            }
            let header = MatterHeaderEntry {
                order: seq,
                matter: state.matter,
                name: event.name().to_owned(),
                kind: event.kind(),
                import: event.import(),
            };
            out.changes
                .push(ViewChange::Upsert(Box::new(ViewEntry::MatterHeader(
                    header.clone(),
                ))));
            state.header = Some(header);
        }
        DomainEvent::CheckpointRecorded(event) => {
            let checkpoint = event.checkpoint();
            if state.versions.contains_key(&checkpoint) {
                out.anomalies.push(Anomaly::Duplicate {
                    event: "checkpoint_recorded",
                    target: EntryId::Version(checkpoint),
                });
                return;
            }
            let entry = ViewEntry::Version(VersionEntry {
                order: seq,
                checkpoint,
                kind: event.kind(),
                thread: event.thread(),
                turn: event.turn(),
                ts: envelope.ts,
                files: Vec::new(),
            });
            state.versions.insert(checkpoint, seq);
            out.changes
                .push(ViewChange::Upsert(Box::new(entry.clone())));
            state.entries.insert(seq, entry);
        }
        DomainEvent::OutsideEdits(event) => {
            let found = state.versions.get(&event.checkpoint).copied();
            let Some(ViewEntry::Version(version)) =
                found.and_then(|order| state.entries.get_mut(&order))
            else {
                out.anomalies.push(Anomaly::UnknownReference {
                    event: "outside_edits",
                    target: EntryId::Version(event.checkpoint),
                });
                return;
            };
            if version.files != event.files {
                event.files.clone_into(&mut version.files);
                out.changes
                    .push(ViewChange::Upsert(Box::new(ViewEntry::Version(
                        version.clone(),
                    ))));
            }
        }
        DomainEvent::VersionRestored(event) => {
            let entry = ViewEntry::Marker(MarkerEntry {
                order: seq,
                event: envelope.event_id,
                kind: MarkerKind::VersionRestored {
                    from: event.from,
                    checkpoint: event.checkpoint,
                    scope: event.scope.clone(),
                },
            });
            out.changes
                .push(ViewChange::Upsert(Box::new(entry.clone())));
            state.entries.insert(seq, entry);
        }
        DomainEvent::ThreadCreated(_)
        | DomainEvent::ThreadRenamed(_)
        | DomainEvent::ThreadArchived(_)
        | DomainEvent::AgentSessionAttached(_)
        | DomainEvent::AgentSessionDetached(_)
        | DomainEvent::UserMessage(_)
        | DomainEvent::TurnQueued(_)
        | DomainEvent::QueuedPromptEdited(_)
        | DomainEvent::SteerResolved(_)
        | DomainEvent::TurnStarted(_)
        | DomainEvent::TextAppended(_)
        | DomainEvent::ContentAppended(_)
        | DomainEvent::ToolCallStarted(_)
        | DomainEvent::ToolCallUpdated(_)
        | DomainEvent::PermissionRequested(_)
        | DomainEvent::PermissionResolved(_)
        | DomainEvent::PermissionExpired(_)
        | DomainEvent::ElicitationRequested(_)
        | DomainEvent::ElicitationResolved(_)
        | DomainEvent::ElicitationExpired(_)
        | DomainEvent::PlanReplaced(_)
        | DomainEvent::ModeChanged(_)
        | DomainEvent::ConfigOptionsReplaced(_)
        | DomainEvent::CommandsAvailable(_)
        | DomainEvent::UsageUpdated(_)
        | DomainEvent::Notice(_)
        | DomainEvent::CompactionChanged(_)
        | DomainEvent::FilesChanged(_)
        | DomainEvent::TurnFailed(_)
        | DomainEvent::TurnStoppedAtLimit(_)
        | DomainEvent::TurnEnded(_)
        | DomainEvent::StepUndone(_) => {
            out.anomalies.push(Anomaly::WrongStreamKind {
                event: thread_kind(&envelope.event),
            });
        }
        DomainEvent::Unknown { .. } => {}
    }
}

/// The kind of a known event, as a `'static` string: `DomainEvent::kind` borrows the event,
/// though a known kind's text is a literal, so it is looked up in `KINDS`.
fn thread_kind(event: &DomainEvent) -> &'static str {
    let kind = event.kind();
    DomainEvent::KINDS
        .iter()
        .copied()
        .find(|known| *known == kind)
        .unwrap_or("unknown")
}
