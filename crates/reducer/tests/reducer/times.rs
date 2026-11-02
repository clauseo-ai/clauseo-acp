//! Clock times on entries: each `ts` and `answered_at` is the `ts` of the envelope that made or
//! settled its entry, never a time made up from `seq`, and snapshots keep them.

use std::collections::BTreeMap;

use clauseo_protocol::{
    CompactionChanged, CompactionStatus, ElicitationExpired, ElicitationOutcome,
    ElicitationRequest, ElicitationRequested, ElicitationResolved, Envelope, ExpiryReason,
    LimitStopReason, Notice, NoticeLevel, PermissionExpired, Principal, Seq, StepUndone, Timestamp,
    TurnStoppedAtLimit, ViewEntry,
};
use clauseo_reducer::{
    Error, REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, ViewChange, apply,
};

use crate::fixtures::{
    Log, doc_path, files_changed, matter_versions, permission_requested, permission_resolved,
    start_turn, thread_created, turn_ended,
};
use crate::helpers::{fold, round_trip};

/// A clock time for `seq` that no rule on `seq` gives: the fixtures' `seq * 1000` is replaced
/// so a fold that took a time from the order, or from another event, shows a wrong value.
#[cfg(test)]
fn clock(seq: Seq) -> Timestamp {
    let n = i64::try_from(seq.get()).unwrap();
    Timestamp::new(1_760_000_000_000 + n * n * 37 + 11).unwrap()
}

/// `events` with each `ts` replaced by [`clock`] of its `seq`.
#[cfg(test)]
fn retimed(events: Vec<Envelope>) -> Vec<Envelope> {
    events
        .into_iter()
        .map(|mut envelope| {
            envelope.ts = clock(envelope.seq);
            envelope
        })
        .collect()
}

/// The `seq`s of the events that settle each entry, by the order of the entry they settle.
#[cfg(test)]
struct Settled {
    permission: (Seq, Seq),
    expired_permission: Seq,
    elicitation: (Seq, Seq),
    expired_elicitation: Seq,
    pending_elicitation: Seq,
}

/// A thread with an entry of every kind that carries a time: a prompt, a permission answered
/// and one expired, elicitations answered, expired and pending, a notice, a compaction marker,
/// a step, a limit stop and an undo marker. Returns the retimed log and which event settled
/// what.
#[cfg(test)]
fn timed_thread() -> (Vec<Envelope>, Settled) {
    let mut log = Log::thread(31).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Draft the notice.").unwrap();
    let ask = |log: &mut Log| {
        let request = log.fresh().unwrap();
        let asked = log.push(permission_requested(request, None)).unwrap().seq;
        (request, asked)
    };
    let (request, asked) = ask(&mut log);
    let answered = log.push(permission_resolved(request)).unwrap().seq;
    let (expiring, expired_permission) = ask(&mut log);
    let reason = ExpiryReason::TurnEnded;
    log.push(PermissionExpired {
        request: expiring,
        reason,
    })
    .unwrap();
    let elicit = |log: &mut Log| {
        let elicitation = log.fresh().unwrap();
        let request = ElicitationRequest::Url {
            message: "Sign in to the court portal.".to_owned(),
            url: "https://example.org/login".to_owned(),
        };
        let seq = log
            .push(ElicitationRequested {
                elicitation,
                request,
            })
            .unwrap()
            .seq;
        (elicitation, seq)
    };
    let (answered_one, elicited) = elicit(&mut log);
    let (expiring_one, expired_elicitation) = elicit(&mut log);
    let (_, pending_elicitation) = elicit(&mut log);
    let elicitation_answered = log
        .push(ElicitationResolved {
            elicitation: answered_one,
            outcome: ElicitationOutcome::Accepted,
            by: Principal::LocalDesktop,
        })
        .unwrap()
        .seq;
    log.push(ElicitationExpired {
        elicitation: expiring_one,
        reason,
    })
    .unwrap();
    let text = "Context is nearly full.".to_owned();
    log.push(Notice {
        level: NoticeLevel::Warning,
        text,
    })
    .unwrap();
    let status = CompactionStatus::Completed;
    log.push(CompactionChanged {
        status,
        summary: None,
    })
    .unwrap();
    log.push(TurnStoppedAtLimit {
        turn,
        window: None,
        resets_at: None,
        reason: LimitStopReason::Temporary,
    })
    .unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    let undo = StepUndone {
        turn,
        checkpoint: log.fresh().unwrap(),
        restored: vec![doc_path(1).unwrap()],
        kept: vec![],
    };
    log.push(undo).unwrap();
    let settled = Settled {
        permission: (asked, answered),
        expired_permission,
        elicitation: (elicited, elicitation_answered),
        expired_elicitation,
        pending_elicitation,
    };
    (retimed(log.into_events()), settled)
}

/// Each entry's time, by order: `ts` for the entries made by one event, `answered_at` for
/// permissions and elicitations. Entries with neither (header, texts, tool calls) are left out.
#[cfg(test)]
fn times(state: &StreamState) -> BTreeMap<Seq, (&'static str, Option<Timestamp>)> {
    state
        .entries()
        .iter()
        .filter_map(|(order, entry)| {
            let time = match entry {
                ViewEntry::UserMessage(e) => Some(e.ts),
                ViewEntry::Step(e) => Some(e.ts),
                ViewEntry::LimitStop(e) => Some(e.ts),
                ViewEntry::Notice(e) => Some(e.ts),
                ViewEntry::Marker(e) => Some(e.ts),
                ViewEntry::Permission(e) => e.answered_at,
                ViewEntry::Elicitation(e) => e.answered_at,
                _ => return None,
            };
            Some((*order, (entry.id().kind(), time)))
        })
        .collect()
}

#[test]
fn each_entry_takes_the_time_of_the_event_that_made_or_settled_it() {
    let (events, settled) = timed_thread();
    let state = fold(&events);
    let got = times(&state);
    let kind_at = |kind: &str| -> Vec<Seq> {
        got.iter()
            .filter(|(_, (k, _))| *k == kind)
            .map(|(order, _)| *order)
            .collect()
    };
    // Entries one event makes carry that event's time, and their order is its seq.
    for kind in ["user_message", "notice", "marker", "limit_stop", "step"] {
        let orders = kind_at(kind);
        assert!(!orders.is_empty(), "no {kind} entry: {got:?}");
        for order in orders {
            assert_eq!(
                got[&order],
                (kind, Some(clock(order))),
                "{kind} at {order:?}"
            );
        }
    }
    assert_eq!(kind_at("marker").len(), 2, "compaction and undo: {got:?}");
    // An answer's time is its resolution's, not its request's; expired and pending have none.
    let (asked, answered) = settled.permission;
    assert_eq!(got[&asked], ("permission", Some(clock(answered))));
    assert_ne!(clock(asked), clock(answered));
    assert_eq!(got[&settled.expired_permission], ("permission", None));
    let (elicited, answered) = settled.elicitation;
    assert_eq!(got[&elicited], ("elicitation", Some(clock(answered))));
    assert_eq!(got[&settled.expired_elicitation], ("elicitation", None));
    assert_eq!(got[&settled.pending_elicitation], ("elicitation", None));
}

#[test]
fn a_settled_answer_sends_its_time_in_the_upsert() {
    let mut log = Log::thread(32).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    start_turn(&mut log, "Check the clause.").unwrap();
    let request = log.fresh().unwrap();
    log.push(permission_requested(request, None)).unwrap();
    log.push(permission_resolved(request)).unwrap();
    let events = retimed(log.into_events());
    let (resolved, before) = events.split_last().unwrap();
    let mut state = fold(before);
    let applied = apply(&mut state, resolved).unwrap();
    let sent = applied.changes.iter().find_map(|change| match change {
        ViewChange::Upsert(entry) => match entry.as_ref() {
            ViewEntry::Permission(p) => Some(p.answered_at),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(sent, Some(Some(clock(resolved.seq))), "{applied:?}");
}

#[test]
fn a_step_shows_when_its_turn_ended_and_a_turn_that_never_ended_has_no_step() {
    let mut log = Log::thread(33).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let running = fold(&retimed(log.events().to_vec()));
    let steps = |state: &StreamState| -> Vec<(Seq, Timestamp)> {
        state
            .entries()
            .values()
            .filter_map(|entry| match entry {
                ViewEntry::Step(step) => Some((step.order, step.ts)),
                _ => None,
            })
            .collect()
    };
    // Still running (or cut off by a crash before start-up repair): no step, so no time.
    assert_eq!(steps(&running), vec![]);
    let checkpoint = log.fresh().unwrap();
    let ended = log.push(turn_ended(turn, checkpoint).unwrap()).unwrap().seq;
    // The undo updates the step; its time stays the end's, the undo's is on its marker.
    let undo = StepUndone {
        turn,
        checkpoint: log.fresh().unwrap(),
        restored: vec![],
        kept: vec![],
    };
    log.push(undo).unwrap();
    let state = fold(&retimed(log.into_events()));
    assert_eq!(steps(&state), vec![(ended, clock(ended))]);
    let step_undone = state.entries().values().any(
        |entry| matches!(entry, ViewEntry::Step(step) if step.undone && step.ts == clock(ended)),
    );
    assert!(step_undone, "{:?}", state.entries());
}

#[test]
fn a_restored_version_marker_takes_its_event_time() {
    let events = retimed(matter_versions().unwrap());
    let state = fold(&events);
    let markers: Vec<_> = state
        .entries()
        .values()
        .filter_map(|entry| match entry {
            ViewEntry::Marker(marker) => Some((marker.order, marker.ts)),
            _ => None,
        })
        .collect();
    assert!(!markers.is_empty(), "{:?}", state.entries());
    for (order, ts) in markers {
        assert_eq!(ts, clock(order));
    }
}

#[test]
fn snapshots_keep_the_times_and_older_snapshots_are_refused_for_replay() {
    let (events, _) = timed_thread();
    let state = fold(&events);
    let read = round_trip(&state);
    assert_eq!(times(&read), times(&state));
    assert_eq!(read, state);
    // A snapshot written before the times (schema 1) is refused by its schema, so the host folds
    // the log instead and every time comes from its event.
    let old = SnapshotRef {
        reducer_schema: 1,
        state: &state,
    };
    let refused = Snapshot::from_bytes(&old.to_bytes().unwrap());
    assert!(
        matches!(refused, Err(Error::SchemaMismatch { found: 1, expected })
            if expected == REDUCER_SCHEMA_VERSION),
        "{refused:?}"
    );
    // An entry without its time does not decode as this schema: the bump is what keeps an old
    // snapshot from failing as garbage.
    let mut value = serde_json::to_value(Snapshot::of(&state)).unwrap();
    let entries = value["state"]["thread"]["entries"].as_array_mut().unwrap();
    let notice = entries
        .iter_mut()
        .find_map(|entry| entry.get_mut("notice"))
        .unwrap();
    notice.as_object_mut().unwrap().remove("ts").unwrap();
    let bytes = rmp_serde::to_vec_named(&value).unwrap();
    let read = Snapshot::from_bytes(&bytes);
    assert!(matches!(read, Err(Error::SnapshotDecode(_))), "{read:?}");
}
