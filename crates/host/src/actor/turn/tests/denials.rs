//! Denials on the turn path with a real `fake_agent`: a failed tool call with "Access is denied"
//! outside the jail gives one `DenialRecorded` (source `tool_call`) that a sandbox report of the
//! same path only counts; a failure inside the jail, or for another reason, gives none; three
//! refusals pause the turn once; the block list is left to its own path; `DismissDenial`.

use std::path::PathBuf;

use clauseo_protocol::{
    CommandOutcome, DenialKind, DenialRecorded, DenialSource, DomainEvent, HostCommand, StopReason,
    ThreadId, TurnId,
};
use clauseo_sandbox::{Access, Denial, DenialSink, HostRefusal, PathClass};

use super::agents::send_prompt;
use super::rig::{Rig, kind};
use crate::actor::turn::TurnPhase;
use crate::actor::turn::denials::feed::DenialFeed;
use crate::config::SessionKey;

/// A path in no matter and not in the data folder, which need not exist: its folder is offered.
fn outside(test: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("clauseo-r18-{test}-{}", std::process::id()))
        .join("notes.docx")
}

/// A tool call reading `path` that fails with `error`, a few words, then the turn waits for a
/// cancel.
fn failing_call(path: &str, error: &str) -> String {
    let (path, error) = (
        serde_json::to_string(path).unwrap(),
        serde_json::to_string(error).unwrap(),
    );
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"tc-1","title":"Read","kind":"read","status":"pending","rawInput":{{"file_path":{path}}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"failed","content":[{{"type":"content","content":{{"type":"text","text":{error}}}}}]}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Could not read it."}}}}}}}}
{CANCEL}"#
    )
}

/// The rest of a turn the lawyer (or a pause) cancels.
const CANCEL: &str = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

/// A prompt that answers a few words, then waits for a cancel.
fn waiting() -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Working."}}}}}}}}
{CANCEL}"#
    )
}

fn recorded(rig: &mut Rig, thread: ThreadId) -> Vec<DenialRecorded> {
    rig.log(thread)
        .into_iter()
        .filter_map(|event| {
            let DomainEvent::DenialRecorded(recorded) = event else {
                return None;
            };
            Some(*recorded)
        })
        .collect()
}

/// A sandbox sink for `thread`'s launch, as PR-D will build it.
fn feed(rig: &Rig, thread: ThreadId, source: DenialSource) -> DenialFeed {
    let key = SessionKey {
        matter: rig.matter(),
        thread,
        launch: 0,
    };
    DenialFeed::new(rig.actor.post.clone(), key, source)
}

fn path_denial(path: PathBuf, class: PathClass) -> Denial {
    Denial::Path {
        path,
        access: Access::Read,
        class,
    }
}

/// Handles inputs until `thread`'s turn holds `cards` cards whose counts add to `total`, and
/// nothing is in flight.
fn until_cards(rig: &mut Rig, thread: ThreadId, cards: usize, total: u32) {
    rig.run_until("the denials recorded", |actor| {
        let held = actor.denials.cards(thread);
        held.len() == cards
            && held.iter().map(|(_, n)| n).sum::<u32>() == total
            && actor.work.commits == 0
    });
}

/// Handles inputs until `thread` has no turn and nothing in flight.
fn until_ended(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
}

/// Handles inputs until the prompt is out and the agent's first words are in.
fn until_prompting(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the prompt out", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|c| c.phase == TurnPhase::Prompting && c.first_words.is_some())
    });
}

fn turn_ended(rig: &mut Rig, thread: ThreadId, turn: TurnId) -> Option<StopReason> {
    rig.log(thread).into_iter().find_map(|event| {
        let DomainEvent::TurnEnded(ended) = event else {
            return None;
        };
        (ended.turn == turn).then_some(ended.stop)
    })
}

#[test]
fn a_failed_tool_call_outside_the_jail_is_one_card_a_sandbox_repeat_counts() {
    let path = outside("tool-call");
    let steps = failing_call(
        &path.display().to_string(),
        "Access is denied. (os error 5)",
    );
    let mut rig = Rig::start("denial-tool-call", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_cards(&mut rig, thread, 1, 1);

    let first = recorded(&mut rig, thread);
    let [first] = first.as_slice() else {
        panic!("{first:?}");
    };
    assert!(
        matches!(first.source, DenialSource::ToolCall { .. }),
        "{first:?}"
    );
    let folder = dunce::canonicalize(std::env::temp_dir()).unwrap();
    let folder = folder.join(path.parent().unwrap().file_name().unwrap());
    assert_eq!(
        first.kind,
        DenialKind::Folder {
            path: folder.display().to_string(),
            other_matter: None
        }
    );
    assert_eq!(
        (first.turn, first.offered, first.count),
        (Some(turn), true, 1)
    );

    // The OS reports the same file: the same card, counted.
    feed(&rig, thread, DenialSource::Sandbox).denied(path_denial(path, PathClass::Outside));
    until_cards(&mut rig, thread, 1, 2);
    let both = recorded(&mut rig, thread);
    let [_, again] = both.as_slice() else {
        panic!("{both:?}");
    };
    assert_eq!((again.denial, again.count), (first.denial, 2));

    // Dismissed: logged with its receipt. A denial the thread does not hold is refused.
    let dismiss = HostCommand::DismissDenial {
        thread,
        denial: first.denial,
    };
    assert_eq!(rig.dispatch(11, dismiss).unwrap(), CommandOutcome::Done);
    let log = rig.log(thread);
    assert!(
        log.iter()
            .any(|e| matches!(e, DomainEvent::DenialDismissed(d) if d.denial == first.denial)),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    let unknown = HostCommand::DismissDenial {
        thread,
        denial: clauseo_protocol::DenialId::new(uuid::Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0999,
        ))
        .unwrap(),
    };
    let refused = rig.dispatch(12, unknown).unwrap_err();
    assert_eq!(
        refused.code(),
        clauseo_protocol::ErrorCode::CommandInvalid,
        "{refused:?}"
    );
    // The same denial named with another thread is refused: it is looked up in that thread.
    let other = rig.thread_by(14);
    let wrong = HostCommand::DismissDenial {
        thread: other,
        denial: first.denial,
    };
    let refused = rig.dispatch(15, wrong).unwrap_err();
    assert_eq!(
        refused.code(),
        clauseo_protocol::ErrorCode::CommandInvalid,
        "{refused:?}"
    );

    rig.dispatch(13, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_failed_tool_call_on_a_unc_path_is_recorded_never_offered() {
    let unc = r"\\server\share\brief.docx";
    let mut rig = Rig::start("denial-unc", &failing_call(unc, "Access is denied."));
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_cards(&mut rig, thread, 1, 1);
    let cards = recorded(&mut rig, thread);
    let [card] = cards.as_slice() else {
        panic!("{cards:?}");
    };
    assert!(
        matches!(card.source, DenialSource::ToolCall { .. }),
        "{card:?}"
    );
    assert_eq!(
        (&card.kind, card.offered, card.turn),
        (
            &DenialKind::File {
                path: unc.to_owned(),
                other_matter: None
            },
            false,
            Some(turn)
        )
    );
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_failure_inside_the_jail_or_for_another_reason_records_nothing() {
    for (test, path, error) in [
        ("denial-inside", "brief.txt".to_owned(), "Access is denied."),
        (
            "denial-other-error",
            outside("other-error").display().to_string(),
            "No such file or directory",
        ),
    ] {
        let mut rig = Rig::start(test, &failing_call(&path, error));
        let thread = rig.thread();
        let _turn = send_prompt(&mut rig, 10, thread);
        // The agent's words come after the failed call: it has been handled.
        until_prompting(&mut rig, thread);
        rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
        until_ended(&mut rig, thread);
        rig.drain();
        assert_eq!(recorded(&mut rig, thread), Vec::new(), "{test}");
        assert!(rig.actor.denials.cards(thread).is_empty(), "{test}");
        rig.finish();
    }
}

#[test]
fn three_refusals_in_a_row_pause_the_turn_once() {
    let mut rig = Rig::start("denial-pause", &waiting());
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_prompting(&mut rig, thread);
    let sink = feed(&rig, thread, DenialSource::Sandbox);
    let base = outside("pause");
    for n in 0..5 {
        let path = base.with_file_name(format!("f{n}")).join("x.docx");
        sink.denied(path_denial(path, PathClass::Outside));
    }
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let paused: Vec<_> = log
        .iter()
        .filter_map(|event| {
            let DomainEvent::DenialsPaused(paused) = event else {
                return None;
            };
            Some(*paused)
        })
        .collect();
    assert_eq!(
        paused.len(),
        1,
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    assert_eq!((paused[0].turn, paused[0].refused), (turn, 3));
    // `session/cancel` went once: the agent's scenario expects exactly one and answered it.
    assert_eq!(
        turn_ended(&mut rig, thread, turn),
        Some(StopReason::Cancelled)
    );
    rig.finish();
}

#[test]
fn the_block_list_is_left_to_its_own_path() {
    let mut rig = Rig::start("denial-blocked", &waiting());
    let thread = rig.thread();
    let _turn = send_prompt(&mut rig, 10, thread);
    until_prompting(&mut rig, thread);
    let sink = feed(&rig, thread, DenialSource::Proxy);
    sink.denied(Denial::Blocked {
        host: "bad.example".into(),
        port: 443,
        listed: "bad.example".into(),
        tag: clauseo_blocklist::Tag::Malware,
        generated: clauseo_blocklist::Generated::parse("2026-11-01T00:00:00Z").unwrap(),
    });
    sink.denied(Denial::UpstreamUnavailable {
        host: "example.com".into(),
        port: 443,
    });
    // A site the allowlist refused is a card; the two before it are not.
    sink.denied(Denial::Host {
        host: "example.com".into(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    });
    until_cards(&mut rig, thread, 1, 1);
    let cards = recorded(&mut rig, thread);
    let [site] = cards.as_slice() else {
        panic!("{cards:?}");
    };
    assert_eq!(
        (&site.kind, site.offered, &site.source),
        (
            &DenialKind::Site {
                host: "example.com".into(),
                shared_hosting: false
            },
            true,
            &DenialSource::Proxy
        )
    );
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    rig.finish();
}
