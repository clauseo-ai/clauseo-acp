//! Steps the pre-write guard stopped (`updates/guard_blocks.rs`): a refusal the guard recorded
//! in the session's file becomes one `Notice` in the turn, read off the actor at the agent's
//! next update or at the turn's end; the same paper refused again in the turn is told once; a
//! paper the matter does not hold is told unnamed; past eight a turn the rest are one count at
//! its end; a Claude session is never told, since Claude shows the blocked call as a failed step
//! of its own.

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
    format!("\n{}\n", serde_json::to_string(&record).unwrap())
}

fn held(path: &str) -> String {
    record(Reason::HeldOpen, Some(path))
}

/// Appends `text` to the record file of `thread`'s session, as the guard would, once the
/// matter holds each of `papers`.
fn guard_writes(rig: &Rig, thread: ThreadId, papers: &[&str], text: &str) {
    let state = &rig.actor.threads[&thread].turn;
    let matter = &state.agent.as_ref().expect("an agent").folder;
    for paper in papers {
        let paper = matter.join(paper);
        write::create_dir_all(paper.parent().unwrap()).unwrap();
        write::write_file(&paper, b"docx").unwrap();
    }
    let run = state.run.as_ref().expect("a run folder");
    let path = file(&run.browser);
    let mut old = std::fs::read(&path).expect("the record file the launch made");
    old.extend_from_slice(text.as_bytes());
    write::write_file(&path, &old).unwrap();
}

/// Until the turn has ended and the guard's reads have answered.
fn until_told(rig: &mut Rig, thread: ThreadId) {
    until_ended(rig, thread);
    rig.run_until("the guard's reads", |actor| {
        let state = &actor.threads[&thread].turn;
        let busy = state
            .agent
            .as_ref()
            .is_some_and(|agent| agent.guard_blocks.busy());
        !busy && actor.work.commits == 0
    });
}

/// The kinds of the notices in `log`, each with the number of the turn it came in or after.
fn notices(log: &[DomainEvent]) -> Vec<(NoticeKind, usize)> {
    let mut turn = 0;
    let mut found = Vec::new();
    for event in log {
        turn += usize::from(matches!(event, DomainEvent::TurnStarted(_)));
        if let DomainEvent::Notice(notice) = event
            && let Some(kind) = &notice.kind
        {
            found.push((kind.clone(), turn));
        }
    }
    found
}

/// Runs the three turns for an agent of `kind`, the guard writing `second` before the second
/// and `third` before the third, with the matter holding `papers`; the thread's notices.
fn three_turns(
    test: &str,
    kind: Option<AgentKind>,
    papers: &[&str],
    [second, third]: [&str; 2],
) -> Vec<(NoticeKind, usize)> {
    let mut rig = Rig::start(test, &script(&lines()));
    rig.actor.turn_path.kind_for_tests = kind;
    let thread = rig.thread();
    let _first = send_prompt(&mut rig, 10, thread);
    until_told(&mut rig, thread);
    guard_writes(&rig, thread, papers, second);
    let _second = send_prompt(&mut rig, 11, thread);
    until_told(&mut rig, thread);
    guard_writes(&rig, thread, &[], third);
    let _third = send_prompt(&mut rig, 12, thread);
    until_told(&mut rig, thread);
    let log = rig.log(thread);
    rig.finish();
    notices(&log)
}

fn lease() -> NoticeKind {
    NoticeKind::HeldOpenBlocked {
        file: MatterPath::new("Lease.docx").unwrap(),
    }
}

#[test]
fn each_refusal_is_told_once_a_turn_by_its_name_alone() {
    let lease_line = held("Drafts/Lease.docx");
    let folder = record(Reason::CodexFolder, None);
    assert_eq!(
        three_turns(
            "turn-guard-blocks",
            None,
            &["Drafts/Lease.docx"],
            [
                &format!("{lease_line}{lease_line}"),
                &format!("{lease_line}{folder}")
            ],
        ),
        vec![
            // The second turn: once, named without its folder.
            (lease(), 2),
            // The third turn, with no update of its own: at its end, the paper again (a new
            // turn) and the `.codex` folder.
            (lease(), 3),
            (NoticeKind::CodexFolderBlocked, 3),
        ]
    );
}

#[test]
fn a_paper_the_matter_does_not_hold_is_told_unnamed() {
    let forged = held("Drafts/Not here.docx");
    assert_eq!(
        three_turns(
            "turn-guard-blocks-forged",
            None,
            &[],
            [&format!("{forged}{forged}"), ""]
        ),
        vec![(NoticeKind::GuardStepBlocked, 2)]
    );
}

#[test]
fn past_eight_a_turn_the_rest_are_one_count_at_its_end() {
    let names: Vec<String> = (0..11).map(|n| format!("Paper {n}.docx")).collect();
    let papers: Vec<&str> = names.iter().map(String::as_str).collect();
    // Eleven papers, one twice: eight named, the other three counted once at the turn's end.
    let mut lines: String = names.iter().map(|name| held(name)).collect();
    lines.push_str(&held("Paper 0.docx"));
    let told = three_turns("turn-guard-blocks-cap", None, &papers, [&lines, ""]);
    let mut expected: Vec<(NoticeKind, usize)> = names[..8]
        .iter()
        .map(|name| {
            let file = MatterPath::new(name).unwrap();
            (NoticeKind::HeldOpenBlocked { file }, 2)
        })
        .collect();
    expected.push((NoticeKind::GuardStepsMore { more: 3 }, 2));
    assert_eq!(told, expected);
}

#[test]
fn a_claude_session_is_never_told() {
    let lease_line = held("Drafts/Lease.docx");
    assert_eq!(
        three_turns(
            "turn-guard-blocks-claude",
            Some(AgentKind::Claude),
            &["Drafts/Lease.docx"],
            [&lease_line, &lease_line]
        ),
        Vec::new()
    );
}
