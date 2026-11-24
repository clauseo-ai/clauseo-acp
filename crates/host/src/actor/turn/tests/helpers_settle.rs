//! A helper's end from the engine's task frames (`helpers/settle.rs`, `AcpEvent::HelperSettled`):
//! from recorded frames played through the ACP reader (`recorded/claude-settle-*.jsonl`, taken
//! from live runs of claude-agent-acp 0.110.1 with `emitRawSDKMessages` set, the matter folder
//! put as `{{cwd}}`, ids, paths and `init` put neutral and short, a long report cut to its
//! start): a helper that hands back, then settles; under code mode a helper with no handback
//! that settles, a helper stopped by a cancel and a shell command's frames. And by hand: a
//! terminal `task_updated` alone, a handback after a settle, a settle while a handback waits
//! for its update, a settle of no helper, a background helper past its turn that settles in
//! the agent's own turn, and a settle behind the helper's words held for a prompted turn.

use clauseo_acp::{AcpEvent, Origin};
use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, HelperOutcome, HostCommand, PromptBlock,
    StreamId, TextOrBlob, ThreadId, TurnId, ViewEntry,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{
    COMPLETED, DELIVERED, LAUNCH_INPUT, PROMPT, answer, call, feed, in_background, lead, live_turn,
    one_turn, patch, session, stamped, story, words,
};
use super::helpers_ends::open_start;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::helpers as turn_helpers;
use crate::actor::turn::own_turn;

const END_TURN: &str = r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#;

/// The handback's report in `claude-settle-handback.jsonl`, as its input has it.
const RECORDED_REPORT: &str = "Digest of lease.txt (text copy, 44 lines, read in full; no \
     garbling noted)\n\nDocument: Lease Agreement, marked \"(invented test document)\" on line \
     1.\n- Dated 3 March 2025 (line 3).";

/// The paper reader's last words in `claude-settle-code-mode.jsonl`.
const RECORDED_LAST_WORDS: &str = "The first line of lease.txt, verbatim, is:\n\nLEASE \
     AGREEMENT (invented test document)\n\nI read it with the REPL's file reader. The shell \
     route failed because the Bash tool is not available in this context.";

/// Each helper end in log order: its outcome and its whole summary (`-` for none).
fn ends(log: &[DomainEvent]) -> Vec<(HelperOutcome, String)> {
    log.iter()
        .filter_map(|event| {
            let DomainEvent::HelperEnded(ended) = event else {
                return None;
            };
            let summary = match &ended.summary {
                Some(TextOrBlob::Inline(text)) => text.clone(),
                Some(TextOrBlob::Blob(_)) => "<blob>".to_owned(),
                None => "-".to_owned(),
            };
            Some((ended.outcome, summary))
        })
        .collect()
}

/// Whether the thread's view holds no helper open.
fn none_open(actor: &mut Actor, thread: ThreadId) -> bool {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| state.open_helpers() == 0)
}

/// How many helpers the thread's view holds ended.
fn ended(actor: &mut Actor, thread: ThreadId) -> usize {
    let Some(state) = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
    else {
        return 0;
    };
    state
        .entries()
        .values()
        .filter(|entry| matches!(entry, ViewEntry::Helper(helper) if helper.outcome.is_some()))
        .count()
}

/// The engine's settle of the helper the agent knows as `helper`, from `origin`.
fn settled(rig: &mut Rig, thread: ThreadId, origin: Origin, helper: &str, outcome: HelperOutcome) {
    let event = AcpEvent::HelperSettled {
        session: session(),
        origin,
        helper: helper.into(),
        outcome,
    };
    feed(rig, thread, event);
}

/// A background helper `toolu_A` started in `turn`.
fn background_helper(rig: &mut Rig, thread: ThreadId, turn: TurnId) {
    lead(
        rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
    in_background(rig, thread, turn, "toolu_A");
}

/// Live run L6d (no code mode): the helper hands back, then its terminal `task_updated` and its
/// `task_notification` come. The handback ends it `Done` with its report; the settle frames
/// after it change nothing (one end), and neither their summary (a pointer to the handback)
/// nor their copy of the report is used. The `init` frames give nothing.
#[test]
fn the_recorded_handback_ends_the_helper_and_its_settle_frames_change_nothing() {
    let frames = include_str!("recorded/claude-settle-handback.jsonl");
    let mut rig = Rig::start(
        "helpers-settle-handback",
        &format!("{PROMPT}\n{frames}{END_TURN}\n"),
    );
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        story(&log),
        [
            "step toolu_01ReaderLaunch -",
            "start paper-reader call=toolu_01ReaderLaunch model=haiku \
             label=Read lease.txt digest task=true",
            "step toolu_01ReaderRead paper-reader",
            "end paper-reader Done Digest of lease.txt (text copy, 44 lines, read in full; no \
             garbling noted)",
            "turn_ended EndTurn",
        ]
    );
    assert_eq!(
        ends(&log),
        [(HelperOutcome::Done, RECORDED_REPORT.to_owned())]
    );
    rig.finish();
}

/// Live run L7d (code mode, no handback tool): the paper reader reads, says its last words and
/// settles `completed`: it ends `Done` with those words. The slow helper is still at work when
/// the lawyer presses Stop: the cancel ends it `Stopped`, and its `killed` and `stopped` frames
/// after the cancel change nothing. Its own shell command runs as a task too (`local_bash`), and
/// the frames of that task, which name the `Bash` step, end no helper.
#[test]
fn the_recorded_code_mode_helper_ends_done_on_its_settle_with_its_last_words() {
    let frames = include_str!("recorded/claude-settle-code-mode.jsonl");
    let mut rig = Rig::start("helpers-settle-code-mode", &format!("{PROMPT}\n{frames}"));
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the reader's end", |actor| ended(actor, thread) == 1);
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let words_line = format!("words paper-reader: {RECORDED_LAST_WORDS}");
    assert_eq!(
        story(&log),
        [
            "step toolu_cc_slow -",
            "start slow-helper call=toolu_cc_slow model=- label=Slow helper task=true",
            "step toolu_cc_reader -",
            "start paper-reader call=toolu_cc_reader model=haiku label=Read lease.txt task=true",
            "step toolu_01SlowBash slow-helper",
            "step toolu_cc_read paper-reader",
            words_line.as_str(),
            "end paper-reader Done The first line of lease.txt, verbatim, is:",
            "end slow-helper Stopped -",
            "turn_ended Cancelled",
        ]
    );
    assert_eq!(
        ends(&log),
        [
            (HelperOutcome::Done, RECORDED_LAST_WORDS.to_owned()),
            (HelperOutcome::Stopped, "-".to_owned()),
        ]
    );
    rig.finish();
}

/// A terminal `task_updated` with no `task_notification` after it (the adapter may skip one)
/// ends the helper through its task's start: `failed` ends it `Failed`, with no summary.
#[test]
fn a_terminal_task_updated_alone_ends_the_helper() {
    let launch = r#"{"update":{"update":{"_meta":{"claudeCode":{"toolName":"Agent"}},"toolCallId":"toolu_A","sessionUpdate":"tool_call","name":"Agent","title":"Task","kind":"think","status":"pending","rawInput":{"subagent_type":"paper-reader","prompt":"Read."}}}}"#;
    let task_started = r#"{"notify":{"method":"_claude/sdkMessage","params":{"sessionId":"sess-1","message":{"type":"system","subtype":"task_started","task_id":"t-A","tool_use_id":"toolu_A","task_type":"local_agent","prompt":"Read."}},"raw":true}}"#;
    let async_mark = r#"{"update":{"update":{"_meta":{"claudeCode":{"toolResponse":{"status":"async_launched","isAsync":true}}},"toolCallId":"toolu_A","sessionUpdate":"tool_call_update"}}}"#;
    let launched = r#"{"update":{"update":{"toolCallId":"toolu_A","sessionUpdate":"tool_call_update","status":"completed"}}}"#;
    let helper_words = r#"{"update":{"update":{"_meta":{"claudeCode":{"parentToolUseId":"toolu_A"}},"sessionUpdate":"agent_message_chunk","messageId":"m-A","content":{"type":"text","text":"The file is gone."}}}}"#;
    let failed = r#"{"notify":{"method":"_claude/sdkMessage","params":{"sessionId":"sess-1","message":{"type":"system","subtype":"task_updated","task_id":"t-A","patch":{"status":"failed","error":"No such file."}}},"raw":true}}"#;
    let steps = [
        PROMPT,
        launch,
        task_started,
        async_mark,
        launched,
        helper_words,
        failed,
        END_TURN,
    ]
    .map(|line| format!("{line}\n"))
    .concat();
    let mut rig = Rig::start("helpers-settle-updated-alone", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "words paper-reader: The file is gone.",
            "end paper-reader Failed -",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// A settle ends the helper first, `Done` with its last words since its last step (its words
/// before that step are not its report); its handback after that changes nothing: one end.
#[test]
fn a_handback_after_a_settle_changes_nothing() {
    let mut rig = Rig::start("helpers-settle-first", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    background_helper(&mut rig, thread, turn);
    stamped(&mut rig, thread, at, "toolu_A", words("Reading now."));
    stamped(
        &mut rig,
        thread,
        at,
        "toolu_A",
        call("toolu_R", "Read", None),
    );
    // One message in two chunks, as its id says.
    stamped(&mut rig, thread, at, "toolu_A", answer("Term to "));
    stamped(&mut rig, thread, at, "toolu_A", answer("2031."));
    settled(&mut rig, thread, at, "toolu_A", HelperOutcome::Done);
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Digest."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    let landed = patch("toolu_H", None, COMPLETED, Some(DELIVERED));
    lead(&mut rig, thread, turn, landed);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ends(&rig.log(thread)),
        [(HelperOutcome::Done, "Term to 2031.".to_owned())]
    );
    rig.finish();
}

/// A foreground helper `toolu_A` started in `turn`: no background mark, so a turn that ends
/// by itself ends it `Failed` when nothing else did.
fn foreground_helper(rig: &mut Rig, thread: ThreadId, turn: TurnId) {
    lead(
        rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(LAUNCH_INPUT)),
    );
}

/// A settle that comes while the helper's handback call waits for its update ends nothing: the
/// update ends the helper `Done` with the report its `rawInput` carries, not the helper's last
/// words, as with no settle.
#[test]
fn a_settle_while_a_handback_waits_leaves_the_end_to_its_update() {
    let mut rig = Rig::start("helpers-settle-handback-waits", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    foreground_helper(&mut rig, thread, turn);
    stamped(&mut rig, thread, at, "toolu_A", words("Handing back now."));
    let handback = call("toolu_H", "SubagentHandback", None);
    stamped(&mut rig, thread, at, "toolu_A", handback);
    settled(&mut rig, thread, at, "toolu_A", HelperOutcome::Done);
    let report = r#"{"message":"Digest."}"#;
    let landed = patch("toolu_H", Some(report), COMPLETED, Some(DELIVERED));
    lead(&mut rig, thread, turn, landed);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ends(&rig.log(thread)),
        [(HelperOutcome::Done, "Digest.".to_owned())]
    );
    rig.finish();
}

/// A settle while the helper's handback call waits, then an update that says `success: false`:
/// the handback's rule ends nothing on a refused handback, so the helper stays open as with no
/// settle, and the turn that ends by itself ends it `Failed`, with no report.
#[test]
fn a_settle_while_a_handback_waits_then_a_refused_update_ends_as_without_the_settle() {
    let mut rig = Rig::start("helpers-settle-handback-refused", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    foreground_helper(&mut rig, thread, turn);
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Draft."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    settled(&mut rig, thread, at, "toolu_A", HelperOutcome::Done);
    let refused = r#"{"success":false,"message":"Nothing was sent."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(refused)),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ends(&rig.log(thread)),
        [(HelperOutcome::Failed, "-".to_owned())]
    );
    rig.finish();
}

/// A settle naming no helper the host started (the lead's own step, an id never seen, in the
/// turn or with none) ends nothing: the helper stays open and ends once, on its own settle
/// later, `Stopped`.
#[test]
fn a_settle_of_no_helper_changes_nothing() {
    let mut rig = Rig::start("helpers-settle-unknown", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    background_helper(&mut rig, thread, turn);
    settled(&mut rig, thread, at, "tc-0", HelperOutcome::Done);
    settled(&mut rig, thread, at, "toolu_never", HelperOutcome::Failed);
    settled(
        &mut rig,
        thread,
        Origin::Idle,
        "toolu_never",
        HelperOutcome::Done,
    );
    settled(&mut rig, thread, at, "toolu_A", HelperOutcome::Stopped);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        ends(&rig.log(thread)),
        [(HelperOutcome::Stopped, "-".to_owned())]
    );
    rig.finish();
}

/// A background helper still at work when the lead's turn ends by itself stays open; its last
/// words come with no turn live and open the agent's own turn, and its settle frames come
/// behind them: the turn's end carries no end for it, and it ends once, `Done` with those
/// words, in the agent's own turn.
#[test]
fn a_background_helper_past_its_turn_ends_on_its_settle_in_the_agent_s_own_turn() {
    let launch = r#"{"update":{"update":{"_meta":{"claudeCode":{"toolName":"Agent"}},"toolCallId":"toolu_A","sessionUpdate":"tool_call","name":"Agent","title":"Task","kind":"think","status":"pending","rawInput":{"subagent_type":"paper-reader","prompt":"Read."}}}}"#;
    let task_started = r#"{"notify":{"method":"_claude/sdkMessage","params":{"sessionId":"sess-1","message":{"type":"system","subtype":"task_started","task_id":"t-A","tool_use_id":"toolu_A","task_type":"local_agent","is_backgrounded":true,"prompt":"Read."}},"raw":true}}"#;
    let async_mark = r#"{"update":{"update":{"_meta":{"claudeCode":{"toolResponse":{"status":"async_launched","isAsync":true}}},"toolCallId":"toolu_A","sessionUpdate":"tool_call_update"}}}"#;
    let launched = r#"{"update":{"update":{"toolCallId":"toolu_A","sessionUpdate":"tool_call_update","status":"completed"}}}"#;
    let helper_words = r#"{"update":{"update":{"_meta":{"claudeCode":{"parentToolUseId":"toolu_A"}},"sessionUpdate":"agent_message_chunk","messageId":"m-A","content":{"type":"text","text":"Digest."}}}}"#;
    let updated = r#"{"notify":{"method":"_claude/sdkMessage","params":{"sessionId":"sess-1","message":{"type":"system","subtype":"task_updated","task_id":"t-A","patch":{"status":"completed"}}},"raw":true}}"#;
    let notified = r#"{"notify":{"method":"_claude/sdkMessage","params":{"sessionId":"sess-1","message":{"type":"system","subtype":"task_notification","task_id":"t-A","tool_use_id":"toolu_A","status":"completed","summary":"Digest."}},"raw":true}}"#;
    let own_words = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own","content":{"type":"text","text":"The reader is back."}}}}"#;
    let cost = r#"{"update":{"update":{"sessionUpdate":"usage_update","used":1,"size":10,"cost":{"amount":0.01,"currency":"USD"}}}}"#;
    let steps = [
        PROMPT,
        launch,
        task_started,
        async_mark,
        launched,
        END_TURN,
        helper_words,
        updated,
        notified,
        own_words,
        cost,
    ]
    .map(|line| format!("{line}\n"))
    .concat();
    let mut rig = Rig::start("helpers-settle-past-turn", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("both turns' ends", |actor| {
        let state = &actor.threads[&thread].turn;
        let idle = state.current.is_none() && state.closing.is_none() && actor.work.commits == 0;
        none_open(actor, thread) && idle
    });
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step toolu_A -",
            "start paper-reader call=toolu_A model=haiku label=- task=true",
            "turn_ended EndTurn",
            "words paper-reader: Digest.",
            "end paper-reader Done Digest.",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// A helper's words and then its settle come with no turn live while a prompt's turn is
/// starting: both wait for it, and the words are logged first, so the end in that turn carries
/// them as its report.
#[test]
fn a_settle_behind_held_words_ends_the_helper_with_them_in_the_prompted_turn() {
    let second =
        r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}"#;
    let answer = r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#;
    let steps = format!("{}{second}\n{answer}\n", one_turn("end_turn"));
    let mut rig = Rig::start("helpers-settle-held-words", &steps);
    let thread = rig.thread();
    let first = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    turn_helpers::start(&mut rig.actor, thread, first, open_start()).unwrap();
    rig.settle_commits();
    let prompt = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Now the notice.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
    };
    // The prompt's turn waits for its before checkpoint, so no turn is live.
    let accepted = rig
        .dispatch_holding(11, prompt, |_, input| matches!(input, Input::Matter { .. }))
        .unwrap();
    let CommandOutcome::PromptAccepted { .. } = accepted else {
        panic!("{accepted:?}");
    };
    stamped(&mut rig, thread, Origin::Idle, "toolu_A", words("Read it."));
    settled(
        &mut rig,
        thread,
        Origin::Idle,
        "toolu_A",
        HelperOutcome::Done,
    );
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 2);
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        story(&log)[1..],
        [
            "turn_ended EndTurn",
            "start paper-reader call= model=- label=- task=true",
            "words paper-reader: Read it.",
            "end paper-reader Done Read it.",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}
