//! Steps the pre-write guard stopped (`updates/guard_blocks.rs`): a refusal the guard recorded
//! in the session's file becomes one `Notice` in the live turn, before the agent's next update
//! or at the turn's end; the same paper refused again in the turn is told once; a Claude session
//! is never told, since Claude shows the blocked call as a failed step of its own.

use clauseo_protocol::{AgentKind, DomainEvent, MatterPath, NoticeKind, ThreadId, Timestamp};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline_quiet::{chunk, script};
use super::rig::Rig;
use crate::actor::turn::updates::guard_blocks::file;
use crate::pre_write_guard::record::{Agent, Reason, Record};
use crate::write;

/// Three turns: the first answers at once, the second sends a chunk, the third answers at once.
fn lines() -> Vec<String> {
    let prompt = |n: u8| {
        format!(
            r#"{{"expect":{{"method":"session/prompt","as":"p{n}","params":{{"sessionId":"sess-1"}}}}}}"#
        )
    };
    let end =
        |n: u8| format!(r#"{{"respond":{{"to":"p{n}","result":{{"stopReason":"end_turn"}}}}}}"#);
    vec![
        prompt(1),
        end(1),
        prompt(2),
        chunk("I could not change it."),
        end(2),
        prompt(3),
        end(3),
    ]
}

fn record(reason: Reason, path: Option<&str>) -> String {
    let record = Record {
        at: Timestamp::ZERO,
        agent: Agent::Codex,
        tool: "apply_patch".to_owned(),
        reason,
        path: path.map(|path| MatterPath::new(path).unwrap()),
    };
    format!("{}\n", serde_json::to_string(&record).unwrap())
}

/// Appends `text` to the record file of `thread`'s session, as the guard would.
fn guard_writes(rig: &Rig, thread: ThreadId, text: &str) {
    let run = rig.actor.threads[&thread]
        .turn
        .run
        .as_ref()
        .expect("a run folder");
    let path = file(&run.root);
    let mut old = std::fs::read(&path).unwrap_or_default();
    old.extend_from_slice(text.as_bytes());
    write::write_file(&path, &old).unwrap();
}

/// The kinds of the notices in `log`, each with whether the turn's text came before it.
fn notices(log: &[DomainEvent]) -> Vec<(NoticeKind, bool)> {
    let mut text = false;
    let mut found = Vec::new();
    for event in log {
        text |= matches!(event, DomainEvent::TextAppended(_));
        if let DomainEvent::Notice(notice) = event
            && let Some(kind) = &notice.kind
        {
            found.push((kind.clone(), text));
        }
    }
    found
}

/// Runs the three turns for an agent of `kind`, the guard refusing before the second and the
/// third; the thread's notices.
fn three_turns(test: &str, kind: Option<AgentKind>) -> Vec<(NoticeKind, bool)> {
    let mut rig = Rig::start(test, &script(&lines()));
    rig.actor.turn_path.kind_for_tests = kind;
    let thread = rig.thread();
    let _first = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let held = record(Reason::HeldOpen, Some("Drafts/Lease.docx"));
    guard_writes(&rig, thread, &format!("{held}{held}"));
    let _second = send_prompt(&mut rig, 11, thread);
    until_ended(&mut rig, thread);
    guard_writes(
        &rig,
        thread,
        &format!("{held}{}", record(Reason::CodexFolder, None)),
    );
    let _third = send_prompt(&mut rig, 12, thread);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    rig.finish();
    notices(&log)
}

#[test]
fn each_refusal_is_told_once_a_turn_before_the_next_update_or_at_the_end() {
    let lease = NoticeKind::HeldOpenBlocked {
        file: MatterPath::new("Drafts/Lease.docx").unwrap(),
    };
    assert_eq!(
        three_turns("turn-guard-blocks", None),
        vec![
            // The second turn: once, before its text.
            (lease.clone(), false),
            // The third turn, with no update of its own: at its end, the paper again (a new
            // turn) and the `.codex` folder.
            (lease, true),
            (NoticeKind::CodexFolderBlocked, true),
        ]
    );
}

#[test]
fn a_claude_session_is_never_told() {
    assert_eq!(
        three_turns("turn-guard-blocks-claude", Some(AgentKind::Claude)),
        Vec::new()
    );
}
