//! Denials and widening in the fold (R18): a refusal's entry and its card's state, a repeat
//! counted on one entry, the dialog's answer, a dismissed card, a paused answer, the cards an
//! exit expires and a restart keeps (with `restarting_since`), and the matter header's
//! remembered answers, read grants and protection switch. A replay gives the same state.

use clauseo_protocol::{
    AgentCapabilities, AgentSessionAttached, AgentSessionDetached, AllowTarget, AllowanceForgotten,
    AllowanceId, AllowanceRecorded, DenialDismissed, DenialEntry, DenialId, DenialKind,
    DenialRecorded, DenialSource, DenialState, DenialsPaused, DetachReason, DomainEvent, EntryId,
    EscalationAnswer, EscalationAnswered, GrantMode, MatterCreated, MatterHeaderEntry, MatterKind,
    ProtectionSwitched, ReadGrantForgotten, ReadGrantedTo, ThreadStatus, TurnId, ViewEntry,
};
use clauseo_reducer::{Anomaly, Applied, StreamState, ViewChange, apply};

use crate::fixtures::{Log, id, start_turn, thread_created};
use crate::helpers::{fold, round_trip};

/// Pushes `event` and applies it, returning what the apply gave.
#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let envelope = log.push(event).unwrap();
    apply(state, &envelope).unwrap()
}

/// A thread `n`, created, with a running turn.
#[cfg(test)]
fn running(n: u64) -> (Log, StreamState, TurnId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the deals folder.").unwrap();
    let state = fold(log.events());
    (log, state, turn)
}

#[cfg(test)]
fn folder() -> DenialKind {
    DenialKind::Folder {
        path: "/Users/a/Deals".to_owned(),
        other_matter: None,
    }
}

#[cfg(test)]
fn refused(turn: TurnId, denial: DenialId, offered: bool, count: u32) -> DenialRecorded {
    DenialRecorded {
        turn: Some(turn),
        denial,
        kind: folder(),
        source: DenialSource::Sandbox,
        offered,
        count,
    }
}

/// The denial's entry in `state`.
#[cfg(test)]
fn entry(state: &StreamState, denial: DenialId) -> DenialEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::Denial(found) if found.denial == denial => Some(found.clone()),
            _ => None,
        })
        .unwrap()
}

#[cfg(test)]
fn answer(denial: DenialId, request: u64, answer: EscalationAnswer) -> EscalationAnswered {
    EscalationAnswered {
        request: id(request).unwrap(),
        denial: Some(denial),
        answer,
    }
}

#[test]
fn an_offered_refusal_is_pending_and_a_repeat_only_counts() {
    let (mut log, mut state, turn) = running(81);
    let denial: DenialId = log.fresh().unwrap();
    let first = push(&mut log, &mut state, refused(turn, denial, true, 1));
    assert!(first.anomalies.is_empty(), "{:?}", first.anomalies);
    assert_eq!(first.changes.len(), 1);
    let shown = entry(&state, denial);
    assert_eq!(shown.state, DenialState::Pending);
    assert_eq!(
        (shown.count, shown.turn, shown.answer),
        (1, Some(turn), None)
    );
    let order = shown.order;

    let again = push(&mut log, &mut state, refused(turn, denial, true, 3));
    assert!(again.anomalies.is_empty(), "{:?}", again.anomalies);
    let shown = entry(&state, denial);
    assert_eq!((shown.count, shown.order), (3, order));
    assert_eq!(
        state
            .entries()
            .values()
            .filter(|e| matches!(e, ViewEntry::Denial(_)))
            .count(),
        1
    );
    // The same count again changes nothing.
    let same = push(&mut log, &mut state, refused(turn, denial, true, 3));
    assert!(same.changes.is_empty(), "{:?}", same.changes);
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn a_refusal_not_offered_has_no_card() {
    let (mut log, mut state, turn) = running(82);
    let denial: DenialId = log.fresh().unwrap();
    push(&mut log, &mut state, refused(turn, denial, false, 1));
    assert_eq!(entry(&state, denial).state, DenialState::NotOffered);
    // An exit leaves it as it is: only a pending card expires.
    push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::Closed,
        },
    );
    assert_eq!(entry(&state, denial).state, DenialState::NotOffered);
}

#[test]
fn the_answer_settles_its_card() {
    let (mut log, mut state, turn) = running(83);
    let cases = [
        (EscalationAnswer::AllowOnce, DenialState::Allowed),
        (EscalationAnswer::AllowForMatter, DenialState::Allowed),
        (EscalationAnswer::Refuse, DenialState::Refused),
        (EscalationAnswer::RefuseForMatter, DenialState::Refused),
        (EscalationAnswer::Unknown, DenialState::Refused),
    ];
    for (n, (given, to)) in (1..).zip(cases) {
        let denial: DenialId = log.fresh().unwrap();
        push(&mut log, &mut state, refused(turn, denial, true, 1));
        let out = push(&mut log, &mut state, answer(denial, n, given));
        assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
        assert_eq!(out.changes.len(), 1);
        let shown = entry(&state, denial);
        assert_eq!((shown.state, shown.answer), (to, Some(given)));
    }
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_matter_level_answer_names_no_card_and_changes_nothing() {
    let (mut log, mut state, _) = running(84);
    let before = state.clone();
    let request = log.fresh().unwrap();
    let out = push(
        &mut log,
        &mut state,
        EscalationAnswered {
            request,
            denial: None,
            answer: EscalationAnswer::AllowForMatter,
        },
    );
    assert!(
        out.changes.is_empty() && out.anomalies.is_empty(),
        "{out:?}"
    );
    assert_eq!(state.entries(), before.entries());
}

#[test]
fn an_answer_or_dismissal_of_an_unknown_denial_is_reported() {
    let (mut log, mut state, _) = running(85);
    let denial: DenialId = log.fresh().unwrap();
    let answered = push(
        &mut log,
        &mut state,
        answer(denial, 9, EscalationAnswer::AllowOnce),
    );
    let dismissed = push(&mut log, &mut state, DenialDismissed { denial });
    for (out, event) in [
        (answered, "escalation_answered"),
        (dismissed, "denial_dismissed"),
    ] {
        assert!(out.changes.is_empty(), "{:?}", out.changes);
        assert_eq!(
            out.anomalies,
            vec![Anomaly::UnknownReference {
                event,
                target: EntryId::Denial(denial),
            }]
        );
    }
}

/// A tool call's claim not offered, then the OS report that confirms it, logged under the same
/// denial: one entry, now `Pending`, with the OS report's kind and source. An answered or
/// dismissed card keeps its state.
#[test]
fn a_card_not_offered_logged_again_offered_is_one_pending_card() {
    let (mut log, mut state, turn) = running(89);
    let denial: DenialId = log.fresh().unwrap();
    let claim = DenialRecorded {
        kind: DenialKind::File {
            path: "/Users/a/Deals/spa.docx".to_owned(),
            other_matter: None,
        },
        source: DenialSource::ToolCall {
            tool_call: id(5).unwrap(),
        },
        ..refused(turn, denial, false, 1)
    };
    push(&mut log, &mut state, claim);
    assert_eq!(entry(&state, denial).state, DenialState::NotOffered);
    let out = push(&mut log, &mut state, refused(turn, denial, true, 2));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(out.changes.len(), 1);
    let shown = entry(&state, denial);
    assert_eq!(
        (shown.state, shown.kind, shown.source, shown.count),
        (DenialState::Pending, folder(), DenialSource::Sandbox, 2)
    );
    let denials = state.entries().values();
    assert_eq!(
        denials
            .filter(|e| matches!(e, ViewEntry::Denial(_)))
            .count(),
        1
    );
    // Dismissed, it stays dismissed however it is logged again.
    push(&mut log, &mut state, DenialDismissed { denial });
    push(&mut log, &mut state, refused(turn, denial, true, 3));
    assert_eq!(entry(&state, denial).state, DenialState::Dismissed);
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn a_dismissed_card_is_dismissed() {
    let (mut log, mut state, turn) = running(86);
    let denial: DenialId = log.fresh().unwrap();
    push(&mut log, &mut state, refused(turn, denial, true, 1));
    let out = push(&mut log, &mut state, DenialDismissed { denial });
    assert_eq!(out.changes.len(), 1);
    assert_eq!(entry(&state, denial).state, DenialState::Dismissed);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_paused_answer_is_an_entry_of_its_own() {
    let (mut log, mut state, turn) = running(87);
    let out = push(&mut log, &mut state, DenialsPaused { turn, refused: 3 });
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let paused: Vec<_> = state
        .entries()
        .values()
        .filter_map(|e| match e {
            ViewEntry::DenialsPaused(p) => Some((p.turn, p.refused)),
            _ => None,
        })
        .collect();
    assert_eq!(paused, vec![(turn, 3)]);
    assert!(matches!(out.changes.as_slice(), [ViewChange::Upsert(_)]));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn an_exit_expires_pending_cards_and_a_restart_keeps_them() {
    let (mut log, mut state, turn) = running(88);
    let denial: DenialId = log.fresh().unwrap();
    push(&mut log, &mut state, refused(turn, denial, true, 1));

    let restart = push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::Restarting,
        },
    );
    assert!(restart.anomalies.is_empty(), "{:?}", restart.anomalies);
    let header = state.as_thread().unwrap().header().unwrap().clone();
    let at = log.events().last().unwrap().ts;
    assert_eq!(header.restarting_since, Some(at));
    assert_eq!(header.status, ThreadStatus::Running);
    assert_eq!(entry(&state, denial).state, DenialState::Pending);
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);

    // The next attach ends the restart.
    push(
        &mut log,
        &mut state,
        AgentSessionAttached {
            session: "sess-2".parse().unwrap(),
            agent_version: None,
            capabilities: AgentCapabilities::default(),
        },
    );
    let header = state.as_thread().unwrap().header().unwrap();
    assert_eq!(header.restarting_since, None);
    assert_eq!(entry(&state, denial).state, DenialState::Pending);

    // A restart, then an exit: the card expires and the restart ends.
    push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::Restarting,
        },
    );
    push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::Exited {
                code: Some(1),
                stderr_tail: String::new(),
            },
        },
    );
    let header = state.as_thread().unwrap().header().unwrap();
    assert_eq!(header.restarting_since, None);
    assert_eq!(entry(&state, denial).state, DenialState::Expired);
    assert_eq!(state, fold(log.events()));
}

/// A matter `n`, created, and its state folded.
#[cfg(test)]
fn matter(n: u64) -> (Log, StreamState) {
    let mut log = Log::matter(n).unwrap();
    let created = MatterCreated::new("Smith v Jones", MatterKind::Scratch, None).unwrap();
    log.push(created).unwrap();
    let state = fold(log.events());
    (log, state)
}

#[cfg(test)]
fn header(state: &StreamState) -> MatterHeaderEntry {
    state.as_matter().unwrap().header().unwrap().clone()
}

#[cfg(test)]
fn site(allowance: AllowanceId, host: &str) -> AllowanceRecorded {
    AllowanceRecorded {
        allowance,
        target: AllowTarget::Site {
            host: host.to_owned(),
        },
        mode: GrantMode::ReadOnly,
        allowed: true,
    }
}

#[test]
fn an_allowance_is_listed_until_forgotten() {
    let (mut log, mut state) = matter(91);
    let (first, second): (AllowanceId, AllowanceId) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(&mut log, &mut state, site(first, "example.com"));
    let out = push(&mut log, &mut state, site(second, "example.org"));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(out.changes.len(), 1);
    let listed = header(&state).allowances;
    assert_eq!(
        listed.iter().map(|a| a.allowance).collect::<Vec<_>>(),
        vec![first, second]
    );
    let first_at = log.events().get(1).unwrap().ts;
    let matter_id = state.as_matter().unwrap().matter();
    assert_eq!(
        listed.first().map(|a| (a.matter, a.at)),
        Some((matter_id, first_at))
    );
    assert_eq!(round_trip(&state), state);

    let again = push(&mut log, &mut state, site(first, "example.net"));
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    assert_eq!(
        again.anomalies,
        vec![Anomaly::Duplicate {
            event: "allowance_recorded",
            target: EntryId::MatterHeader(matter_id),
        }]
    );

    push(
        &mut log,
        &mut state,
        AllowanceForgotten { allowance: first },
    );
    let listed = header(&state).allowances;
    assert_eq!(
        listed.iter().map(|a| a.allowance).collect::<Vec<_>>(),
        vec![second]
    );
    let gone = push(
        &mut log,
        &mut state,
        AllowanceForgotten { allowance: first },
    );
    assert!(gone.changes.is_empty(), "{:?}", gone.changes);
    assert_eq!(
        gone.anomalies,
        vec![Anomaly::UnknownReference {
            event: "allowance_forgotten",
            target: EntryId::MatterHeader(matter_id),
        }]
    );
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_read_grant_is_listed_until_forgotten() {
    let (mut log, mut state) = matter(92);
    let by_matter = log.fresh().unwrap();
    let allowance = log.fresh().unwrap();
    let grant = ReadGrantedTo {
        by_matter,
        by_name: "Jones estate".to_owned(),
        allowance,
    };
    push(&mut log, &mut state, grant.clone());
    assert_eq!(header(&state).read_grants, vec![grant.clone()]);
    let twice = push(&mut log, &mut state, grant);
    assert!(twice.changes.is_empty(), "{:?}", twice.changes);
    assert_eq!(twice.anomalies.len(), 1);
    push(
        &mut log,
        &mut state,
        ReadGrantForgotten {
            by_matter,
            allowance,
        },
    );
    assert_eq!(header(&state).read_grants, Vec::<ReadGrantedTo>::new());
    assert_eq!(state, fold(log.events()));
}

#[test]
fn protection_switches_off_and_on() {
    let (mut log, mut state) = matter(93);
    assert!(!header(&state).switched_off);
    let off = push(&mut log, &mut state, ProtectionSwitched { off: true });
    assert_eq!(off.changes.len(), 1);
    assert!(header(&state).switched_off);
    let again = push(&mut log, &mut state, ProtectionSwitched { off: true });
    assert!(again.changes.is_empty(), "{:?}", again.changes);
    push(&mut log, &mut state, ProtectionSwitched { off: false });
    assert!(!header(&state).switched_off);
    assert_eq!(state, fold(log.events()));
    assert_eq!(round_trip(&state), state);
}

/// R18's kinds in the other kind of stream change nothing and are reported by their kind.
#[test]
fn each_kind_in_the_wrong_stream_is_reported() {
    let (mut log, mut state, turn) = running(94);
    let allowance: AllowanceId = log.fresh().unwrap();
    let out = push(&mut log, &mut state, site(allowance, "example.com"));
    assert_eq!(
        out.anomalies,
        vec![Anomaly::WrongStreamKind {
            event: "allowance_recorded"
        }]
    );
    let (mut log, mut state) = matter(95);
    let denial: DenialId = log.fresh().unwrap();
    let out = push(&mut log, &mut state, refused(turn, denial, true, 1));
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(out.anomalies.len(), 1);
}
