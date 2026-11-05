//! A notice's kind: the host's notices carry one into the view, for the app's own words; the
//! agent's carry none, and their entry serialises as before kinds.

use clauseo_protocol::{DomainEvent, Notice, NoticeEntry, NoticeKind, NoticeLevel, ViewEntry};
use clauseo_reducer::{
    Error, REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, ViewChange, apply,
};

use crate::fixtures::{Log, thread_created};
use crate::helpers::{fold, round_trip};

/// The words the host sends with `ContextNotRestored`, for a view that does not know the kind.
#[cfg(test)]
const FALLBACK: &str = "Clauseo doesn't remember the earlier questions and answers.";

/// A thread with a host notice of kind `ContextNotRestored`, then an agent notice.
#[cfg(test)]
fn host_then_agent() -> Log {
    let mut log = Log::thread(41).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    log.push(Notice {
        level: NoticeLevel::Info,
        text: FALLBACK.to_owned(),
        kind: Some(NoticeKind::ContextNotRestored),
    })
    .unwrap();
    log.push(Notice {
        level: NoticeLevel::Warning,
        text: "Context is nearly full.".to_owned(),
        kind: None,
    })
    .unwrap();
    log
}

#[cfg(test)]
fn notices(state: &StreamState) -> Vec<&NoticeEntry> {
    state
        .entries()
        .values()
        .filter_map(|entry| match entry {
            ViewEntry::Notice(notice) => Some(notice),
            _ => None,
        })
        .collect()
}

#[test]
fn a_host_notice_keeps_its_kind_and_an_agent_notice_has_none() {
    let log = host_then_agent();
    let state = fold(log.events());
    let found: Vec<_> = notices(&state)
        .into_iter()
        .map(|notice| (notice.kind.clone(), notice.text.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (Some(NoticeKind::ContextNotRestored), FALLBACK),
            (None, "Context is nearly full."),
        ]
    );
    // A snapshot keeps the kind.
    assert_eq!(round_trip(&state), state);
}

#[test]
fn the_upsert_carries_the_kind_and_an_agent_notice_writes_no_kind_field() {
    let mut log = Log::thread(42).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let mut upserted = Vec::new();
    for kind in [Some(NoticeKind::ContextNotRestored), None] {
        let notice = Notice {
            level: NoticeLevel::Info,
            text: FALLBACK.to_owned(),
            kind,
        };
        let out = apply(&mut state, &log.push(notice).unwrap()).unwrap();
        upserted.extend(out.changes.into_iter().filter_map(|change| match change {
            ViewChange::Upsert(entry) => match *entry {
                ViewEntry::Notice(notice) => Some(notice),
                _ => None,
            },
            _ => None,
        }));
    }
    let json: Vec<serde_json::Value> = upserted
        .iter()
        .map(|notice| serde_json::to_value(notice).unwrap())
        .collect();
    assert_eq!(json.len(), 2, "{json:?}");
    assert_eq!(json[0]["kind"], "context_not_restored");
    assert!(json[1].get("kind").is_none(), "{}", json[1]);
}

/// Schema 10 folds a notice's kind into the view: a snapshot schema 9 wrote (a build that
/// dropped the kind) is refused, so the stream is refolded from its log and the kind is there.
#[test]
fn a_schema_9_snapshot_is_refolded() {
    assert_eq!(REDUCER_SCHEMA_VERSION, 10);
    let log = host_then_agent();
    // What a schema 9 build folded from this log: the same notices, with no kind.
    let kindless: Vec<_> = log
        .events()
        .iter()
        .cloned()
        .map(|mut envelope| {
            if let DomainEvent::Notice(notice) = &mut envelope.event {
                notice.kind = None;
            }
            envelope
        })
        .collect();
    let old_state = fold(&kindless);
    assert!(
        notices(&old_state)
            .iter()
            .all(|notice| notice.kind.is_none())
    );
    let old = SnapshotRef {
        reducer_schema: 9,
        state: &old_state,
    };
    let read = Snapshot::from_bytes(&old.to_bytes().unwrap());
    assert!(
        matches!(
            read,
            Err(Error::SchemaMismatch {
                found: 9,
                expected: 10
            })
        ),
        "{read:?}"
    );
    let refolded = fold(log.events());
    assert_eq!(
        notices(&refolded)[0].kind,
        Some(NoticeKind::ContextNotRestored)
    );
}
