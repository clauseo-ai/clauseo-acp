//! Steps the pre-write guard stopped (`updates/guard_blocks.rs`): a refusal the guard recorded
//! in the session's file becomes one `Notice` in the turn, read off the actor at the agent's
//! next update or at the turn's end; the same paper refused again in the turn is told once; a
//! paper the matter does not hold is told unnamed; every step without a notice of its own (a
//! repeat, named or not, or one past eight a turn) is in one count at the turn's end; a Claude
//! session is never told, since Claude shows the blocked call as a failed step of its own. And
//! the no-copy list end to end: the launch makes it, the before-turn checkpoint lists a file over
//! the size cap on it, the real guard refuses its delete and records it, and the next turn tells
//! it.

use clauseo_protocol::{AgentKind, DomainEvent, MatterPath, NoticeKind, ThreadId, Timestamp};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline_quiet::{chunk, script};
use super::rig::Rig;
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
    let (path, _) = run.guard.as_ref().expect("the record the launch made");
    let mut old = std::fs::read(path).expect("the record file the launch made");
    old.extend_from_slice(text.as_bytes());
    // Written in place, so the file keeps the identity the launch read.
    write::write_file(path, &old).unwrap();
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
            // The second turn: once, named without its folder; its repeat in the count at the
            // turn's end.
            (lease(), 2),
            (NoticeKind::GuardStepsMore { more: 1 }, 2),
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
        vec![
            (NoticeKind::GuardStepBlocked, 2),
            (NoticeKind::GuardStepsMore { more: 1 }, 2)
        ]
    );
}

/// The one count at the turn's end is every step stopped that got no notice of its own: the
/// unnamed step and seven of eight distinct papers are told (eight notices), and the eighth
/// paper, two repeats of a named paper and two of the unnamed step make five.
#[test]
fn every_step_without_a_notice_of_its_own_is_in_the_one_count_at_the_turns_end() {
    let names: Vec<String> = (0..8).map(|n| format!("Paper {n}.docx")).collect();
    let papers: Vec<&str> = names.iter().map(String::as_str).collect();
    let forged = held("Drafts/Not here.docx");
    let mut lines = forged.clone();
    lines.extend(names.iter().map(|name| held(name)));
    lines.push_str(&held("Paper 0.docx").repeat(2));
    lines.push_str(&forged.repeat(2));
    let told = three_turns("turn-guard-blocks-cap", None, &papers, [&lines, ""]);
    let mut expected = vec![(NoticeKind::GuardStepBlocked, 2)];
    expected.extend(names[..7].iter().map(|name| {
        let file = MatterPath::new(name).unwrap();
        (NoticeKind::HeldOpenBlocked { file }, 2)
    }));
    expected.push((NoticeKind::GuardStepsMore { more: 5 }, 2));
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

/// The no-copy list the launch made, as it reads now.
fn no_copy_list(
    rig: &Rig,
    thread: ThreadId,
) -> (crate::pre_write_guard::no_copy::NoCopyList, String) {
    let run = rig.actor.threads[&thread]
        .turn
        .run
        .as_ref()
        .expect("a run folder");
    let list = run.no_copy.clone().expect("the list the launch made");
    let text = std::fs::read_to_string(&list.file).unwrap();
    (list, text)
}

#[test]
fn a_file_with_no_copy_is_listed_refused_recorded_and_told() {
    use crate::pre_write_guard::no_copy::HEADER;
    let mut rig = Rig::start("turn-guard-no-copy", &script(&lines()));
    let thread = rig.thread();
    let _first = send_prompt(&mut rig, 10, thread);
    until_told(&mut rig, thread);
    assert_eq!(
        no_copy_list(&rig, thread).1,
        HEADER,
        "nothing without a copy yet"
    );
    let state = &rig.actor.threads[&thread].turn;
    let matter = state.agent.as_ref().expect("an agent").folder.clone();
    let run = state.run.as_ref().expect("a run folder");
    let (record, record_id) = run.guard.clone().expect("the record the launch made");
    assert_eq!(no_copy_list(&rig, thread).0.file.parent(), record.parent());
    // One byte over the cap, sparse: the checkpoint skips it unread.
    write::sized_file(&matter.join("Big.bin"), clauseo_matters::FILE_CAP_BYTES + 1).unwrap();
    let _second = send_prompt(&mut rig, 11, thread);
    until_told(&mut rig, thread);
    let (list, text) = no_copy_list(&rig, thread);
    assert_eq!(text, format!("{HEADER}add over-size-cap Big.bin\n"));
    // The guard as Codex's hook runs it, with the record and the list the launch made.
    let args: Vec<std::ffi::OsString> = [
        "--record".into(),
        record.into_os_string(),
        "--record-id".into(),
        record_id.hex().into(),
        "--no-copy-list".into(),
        list.file.into_os_string(),
        "--no-copy-list-id".into(),
        list.id.hex().into(),
    ]
    .into();
    let hook = serde_json::json!({
        "cwd": matter.to_str().unwrap(),
        "tool_name": "Bash",
        "tool_input": {"command": "rm -f Big.bin"},
    });
    let mut errors = Vec::new();
    let code =
        crate::pre_write_guard::run(&args, hook.to_string().as_bytes(), Vec::new(), &mut errors);
    let said = String::from_utf8(errors).unwrap();
    assert_eq!(code, crate::pre_write_guard::DENY, "{said}");
    assert!(said.starts_with("Big.bin can't be brought back"), "{said}");
    let _third = send_prompt(&mut rig, 12, thread);
    until_told(&mut rig, thread);
    let log = rig.log(thread);
    rig.finish();
    let file = MatterPath::new("Big.bin").unwrap();
    assert_eq!(notices(&log), [(NoticeKind::NoCopyBlocked { file }, 3)]);
}
