//! "Always for this matter" (decision record 0063, `agent/always.rs`): no agent option that
//! would write a rule reaches the lawyer; the host offers its own `clauseo.always` when the
//! request gives a rule it can keep, and the agent gets the request's allow-once option, so no
//! agent writes anything into the matter. The host keeps the rule beside the matter's notes,
//! signed with a key from the secret store, and a later request with the same rule is allowed by
//! the host, logged, in this session and after a relaunch, in forced ask too. No launch carries a
//! kept rule in its flag settings: the agent asks the host, which answers only after its data
//! fence and held-open checks. A planted or copied rules file, or a host with no secret store,
//! keeps nothing. Each scenario checks the option the agent got.

use clauseo_protocol::{
    AgentChoiceId, AlwaysKind, AlwaysOffer, DomainEvent, ErrorCode, FolderSupportKind, HostCommand,
    HostQuery, MatterId, PermissionChoice, PermissionKind, Principal, QueryResult, StopReason,
    StreamId, ThreadId,
};
use std::fmt::Write as _;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{ALWAYS_KEY, Dir, Running, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";

/// The host's "Always for this matter" option.
pub(crate) const ALWAYS: &str = "clauseo.always";

/// The guard program the forced-ask host names, absolute on every system; the fake agent never
/// runs it.
fn guard() -> std::path::PathBuf {
    std::env::temp_dir().join("clauseo-desktop")
}

/// What the lawyer's own `.claude` folder in the matter holds; it must stay as it is.
const OWN_SETTINGS: &str = "{\"permissions\": {\"allow\": [\"Read\"]}}\n";

/// `<data>/agent-state/<matter>/claude-allow.json`.
pub(crate) fn rules_file(dir: &Dir, matter: MatterId) -> std::path::PathBuf {
    dir.data()
        .join("agent-state")
        .join(matter.to_string())
        .join("claude-allow.json")
}

/// HMAC-SHA256 (RFC 2104) of `message` under `key`.
fn hmac(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut pad = [0_u8; 64];
    pad[..32].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(pad.map(|byte| byte ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(pad.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// A rules file holding `rules`, as the host writes one for the matter whose id is
/// `signed_for` (version 2, its MAC over `"v2\0" + matter id + "\0" + rules joined by "\0"`)
/// under the tests' key (`ALWAYS_KEY`).
pub(crate) fn signed_rules(signed_for: &str, rules: &[&str]) -> String {
    let message = format!("v2\0{signed_for}\0{}", rules.join("\0"));
    let mac = hmac(&ALWAYS_KEY, message.as_bytes())
        .iter()
        .fold(String::new(), |mut mac, byte| {
            write!(mac, "{byte:02x}").unwrap();
            mac
        });
    json!({"version": 2, "allow": rules, "mac": mac}).to_string()
}

/// Writes `rules` as `matter`'s kept rules, as the host stores them.
pub(crate) fn keep(dir: &Dir, matter: MatterId, rules: &[&str]) {
    let text = signed_rules(&matter.to_string(), rules);
    crate::write::write(&rules_file(dir, matter), text.as_bytes());
}

/// Waits until `matter`'s rules file holds `n` rules or more (each is kept on a thread of its
/// own).
pub(crate) fn until_kept(dir: &Dir, matter: MatterId, n: usize) {
    let file = rules_file(dir, matter);
    let count = || {
        std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| value.get("allow")?.as_array().map(Vec::len))
            .unwrap_or(0)
    };
    crate::support::wait(&format!("{n} kept rules"), async {
        while count() < n {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
}

/// The rules `matter` keeps, as `KeptPermissions` lists them.
pub(crate) fn kept_rules(host: &Running, matter: MatterId) -> Vec<String> {
    match host.query(HostQuery::KeptPermissions { matter }) {
        Ok(QueryResult::KeptPermissions(kept)) => kept.into_iter().map(|k| k.rule).collect(),
        other => panic!("KeptPermissions answered {other:?}"),
    }
}

/// A request's option ids and its chosen option, if it was resolved.
type Resolution = (Vec<String>, Option<(String, PermissionKind)>);

/// Each request's [`Resolution`], in order; every resolution is the desktop's.
pub(crate) fn resolutions(events: &Events) -> Vec<Resolution> {
    let mut out: Vec<(clauseo_protocol::PermissionRequestId, Vec<String>)> = Vec::new();
    let mut chosen = std::collections::BTreeMap::new();
    for event in events.events() {
        match event {
            DomainEvent::PermissionRequested(asked) => out.push((
                asked.request,
                asked
                    .options
                    .iter()
                    .map(|o| o.option_id.to_string())
                    .collect(),
            )),
            DomainEvent::PermissionResolved(resolved) => {
                assert_eq!(resolved.by, Principal::LocalDesktop);
                if let PermissionChoice::Selected { option_id, kind } = &resolved.choice {
                    chosen.insert(resolved.request, (option_id.to_string(), *kind));
                }
            }
            _ => {}
        }
    }
    out.into_iter()
        .map(|(request, options)| (options, chosen.get(&request).cloned()))
        .collect()
}

/// What each logged request's `clauseo.always` would keep, in order.
pub(crate) fn offers(events: &Events) -> Vec<Option<AlwaysOffer>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(asked.always.clone()),
            _ => None,
        })
        .collect()
}

/// Every logged option of kind `allow_always` is the host's own: `clauseo.always` (labelled
/// "Always for this matter"), a `clauseo.session:` option, or a plan exit's.
pub(crate) fn assert_only_host_always(events: &Events) {
    for event in events.events() {
        let DomainEvent::PermissionRequested(asked) = event else {
            continue;
        };
        for option in &asked.options {
            if option.kind != PermissionKind::AllowAlways {
                continue;
            }
            let id = option.option_id.as_str();
            let host = (id == ALWAYS && option.agent_label == "Always for this matter")
                || id.starts_with("clauseo.session:")
                || id.starts_with("exit-plan-");
            assert!(host, "{option:?} reached a card: {}", events.dump());
        }
    }
}

/// The lawyer's answer `option` to request `index` (0-based) of `thread`, refused `code`.
fn refused_answer(
    host: &Running,
    events: &Events,
    thread: ThreadId,
    index: usize,
    option: &str,
    code: ErrorCode,
) {
    let answer = HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[index],
        option_id: AgentChoiceId::new(option).unwrap(),
    };
    host.refused(answer, code);
}

/// Waits for request `n` (1-based) of the thread `events` follows.
fn asked(events: &mut Events, n: usize) {
    events.until(&format!("permission request {n}"), |e| {
        e.permission_requests().len() >= n
    });
}

/// The card's offer for Claude's `wc -l notes.txt` in the matter folder.
fn wc_offer(command: &str) -> AlwaysOffer {
    AlwaysOffer {
        kind: AlwaysKind::Command,
        shown: Some(command.to_owned()),
        cwd: None,
    }
}

/// claude-agent-acp 0.89.1's options on a Bash call.
fn bash_options() -> Value {
    json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ])
}

/// A Claude scenario named `name`, written into `dir`'s work folder: a claude-acp handshake
/// (in default with no auto mode when `forced`, as a forced-ask launch is; else in auto), one
/// prompt whose steps are `turn`, then `Done.`.
fn claude_scenario(dir: &Dir, name: &str, forced: bool, turn: Vec<Value>) -> Scenario {
    let modes = if forced {
        json!({"currentModeId": "default", "availableModes": [
            {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"},
            {"id": "plan", "name": "Plan"}]})
    } else {
        json!({"currentModeId": "auto", "availableModes": [
            {"id": "default", "name": "Manual"}, {"id": "acceptEdits", "name": "Accept edits"},
            {"id": "plan", "name": "Plan"}, {"id": "auto", "name": "Auto"}]})
    };
    let mut lines = vec![
        json!({"scenario": name, "description": "written by always.rs", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": modes}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(turn);
    lines.push(
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk",
        "messageId": "m-p1", "content": {"type": "text", "text": "Done."}}}}),
    );
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let mut text = String::new();
    for line in &lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    let file = dir.work_path(&format!("{name}.jsonl"));
    crate::write::write(&file, text.as_bytes());
    Scenario::Written(file)
}

#[test]
fn always_for_this_matter_is_kept_by_the_host_not_in_the_matter() {
    let dir = Dir::new("always_kept", &[(CLAUDE, Scenario::Local("claude-always"))]);
    let host = dir.start(0);
    let files = [
        ("brief.txt", "x\n"),
        (".claude/settings.local.json", OWN_SETTINGS),
    ];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let card = ["allow-once", ALWAYS, "reject"].map(String::from).to_vec();
    let host_answered = ["allow-once", "reject"].map(String::from).to_vec();
    assert_eq!(
        resolutions(&events),
        [
            // The agent's "don't ask again" is gone; the host's "Always" takes its place, and
            // the log keeps the lawyer's choice.
            (card, Some((ALWAYS.to_owned(), PermissionKind::AllowAlways))),
            // The second is the host's, from the kept rule.
            (
                host_answered,
                Some(("allow-once".to_owned(), PermissionKind::AllowOnce))
            ),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [Some(wc_offer("wc -l notes.txt")), None]);
    // The rule is kept in Clauseo's data, written on its own thread.
    let file = rules_file(&dir, matter);
    let kept =
        || std::fs::read_to_string(&file).is_ok_and(|text| text.contains("Bash(wc -l notes.txt)"));
    crate::support::wait("the kept rule", async {
        while !kept() {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
    // Nothing in the matter changed: the lawyer's own `.claude` is as it was.
    let own = std::fs::read_dir(folder.join(".claude")).unwrap().count();
    assert_eq!(own, 1);
    let text = std::fs::read_to_string(folder.join(".claude/settings.local.json")).unwrap();
    assert_eq!(text, OWN_SETTINGS);
    host.shutdown();

    // The next launch's settings hold no kept rule, and the host answers the request from it.
    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-always-load"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run them again.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let last = resolutions(&events).pop().and_then(|(_, chosen)| chosen);
    assert_eq!(
        last,
        Some(("allow-once".to_owned(), PermissionKind::AllowOnce)),
        "{}",
        events.dump()
    );
    assert!(!folder.join(".claude/settings.json").exists());
    host.shutdown();
}

/// Forced ask keeps and applies too: a rule the matter kept earlier answers its request with no
/// card and reaches no launch settings, and the lawyer's "Always" on another command keeps it
/// and answers the next one.
#[test]
fn forced_ask_keeps_and_applies_kept_rules() {
    let dir = Dir::new(
        "always_forced",
        &[(CLAUDE, Scenario::Local("claude-always-forced"))],
    );
    let host = dir.start_with(0, |config| {
        config
            .with_folder_support(Some(FolderSupportKind::Network))
            .with_pre_write_guard(guard())
    });
    let (matter, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    keep(&dir, matter, &["Bash(wc -l notes.txt)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    answer_from(&host, &mut events, thread, 1, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let once = Some(("allow-once".to_owned(), PermissionKind::AllowOnce));
    let host_answered = ["allow-once", "reject"].map(String::from).to_vec();
    assert_eq!(
        resolutions(&events),
        [
            (host_answered.clone(), once.clone()),
            (
                ["allow-once", ALWAYS, "reject"].map(String::from).to_vec(),
                Some((ALWAYS.to_owned(), PermissionKind::AllowAlways))
            ),
            (host_answered, once),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events)[1], Some(wc_offer("wc -c notes.txt")));
    until_kept(&dir, matter, 2);
    assert_eq!(
        kept_rules(&host, matter),
        ["Bash(wc -l notes.txt)", "Bash(wc -c notes.txt)"]
    );
    assert!(!folder.join(".claude").exists());
    host.shutdown();
}

/// A kept rule never reaches outside the matter folder, so it never widens past the data fence
/// (`agent::always::within`): path rules the file holds for a sibling folder, or for the
/// matter's `.clauseo`, are dropped at the launch (and left in the file), and an edit of a file
/// outside the matter offers no "Always", so it cannot be kept. "Always" on an edit inside the
/// matter keeps `Edit(*)`, which answers the next change to a file in the matter, by any tool.
#[cfg(unix)]
#[test]
fn a_rule_outside_the_matter_is_neither_kept_nor_applied() {
    let dir = Dir::new(
        "always_fence",
        &[(CLAUDE, Scenario::Local("claude-always-fence"))],
    );
    let host = dir.start(0);
    let files = [("brief.txt", "x\n")];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let root = folder.to_str().unwrap();
    let outside = [
        format!("Read(/{root}-b/a.txt)"),
        format!("Read(/{root}/.clauseo/matter.json)"),
    ];
    keep(
        &dir,
        matter,
        &["Bash(wc -l notes.txt)", &outside[0], &outside[1]],
    );
    let file = rules_file(&dir, matter);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the files.");
    // The sibling's file: no "Always" to answer.
    asked(&mut events, 1);
    refused_answer(
        &host,
        &events,
        thread,
        0,
        ALWAYS,
        ErrorCode::PermissionUnknownOption,
    );
    answer_from(&host, &mut events, thread, 0, &["allow-once", ALWAYS]);
    asked(&mut events, 4);
    answer_from(&host, &mut events, thread, 3, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let chosen: Vec<_> = resolutions(&events)
        .into_iter()
        .map(|(_, chosen)| chosen.map(|(option, _)| option))
        .collect();
    let want = ["allow-once", ALWAYS, "allow-once", "reject"];
    assert_eq!(
        chosen,
        want.map(|o| Some(o.to_owned())),
        "{}",
        events.dump()
    );
    let edit = Some(AlwaysOffer {
        kind: AlwaysKind::FileEdit,
        shown: None,
        cwd: None,
    });
    let offered = offers(&events);
    assert_eq!(
        (&offered[0], &offered[1], &offered[3]),
        (&None, &edit, &None)
    );
    let has = |rule: &str| std::fs::read_to_string(&file).is_ok_and(|text| text.contains(rule));
    crate::support::wait("the kept rule", async {
        while !has("Edit(*)") {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
    // The rules read from the file stay in it; nothing outside the matter was added.
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("-b/a.txt").count(), 1, "{text}");
    assert!(!folder.join(".claude").exists());
    host.shutdown();
}

/// An option an agent sends under a host id (`clauseo.always`, `clauseo.session:x`) is dropped
/// on arrival, in forced ask and outside it: a card shows the host's own `clauseo.always` only
/// where the request gives a rule, and answering a dropped id is refused.
#[test]
fn an_agents_own_host_option_ids_are_dropped() {
    for forced in [false, true] {
        let dir = Dir::new(&format!("always_agent_ids_{forced}"), &[]);
        let options = json!([
            {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
            {"optionId": ALWAYS, "name": "Always (the agent's)", "kind": "allow_always"},
            {"optionId": "clauseo.session:x", "name": "This session", "kind": "allow_once"},
            {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
            {"optionId": "reject", "name": "No", "kind": "reject_once"},
        ]);
        let mut turn = Vec::new();
        let wc = json!({"command": "wc -l notes.txt"});
        turn.extend(ask("one", "Bash", &wc, &options, "allow-once"));
        let ls = json!({"command": "ls *.docx"});
        turn.extend(ask("glob", "Bash", &ls, &options, "allow-once"));
        let scenario = claude_scenario(&dir, "claude-agent-ids", forced, turn);
        dir.set_agents(&[(CLAUDE, scenario)]);
        let host = dir.start_with(0, |config| {
            if forced {
                config
                    .with_folder_support(Some(FolderSupportKind::Network))
                    .with_pre_write_guard(guard())
            } else {
                config
            }
        });
        let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Count the lines.");
        asked(&mut events, 1);
        refused_answer(
            &host,
            &events,
            thread,
            0,
            "clauseo.session:x",
            ErrorCode::PermissionUnknownOption,
        );
        answer_from(&host, &mut events, thread, 0, &["allow-once"]);
        asked(&mut events, 2);
        for dropped in [ALWAYS, "clauseo.session:x", "allow-with-updates"] {
            refused_answer(
                &host,
                &events,
                thread,
                1,
                dropped,
                ErrorCode::PermissionUnknownOption,
            );
        }
        answer_from(&host, &mut events, thread, 1, &["allow-once"]);
        let ended = events.until_turn_ended(turn);
        events.assert_no_mismatch();
        assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
        let cards: Vec<_> = resolutions(&events)
            .into_iter()
            .map(|(card, _)| card)
            .collect();
        assert_eq!(
            cards,
            [
                vec![
                    "allow-once".to_owned(),
                    ALWAYS.to_owned(),
                    "reject".to_owned()
                ],
                vec!["allow-once".to_owned(), "reject".to_owned()],
            ],
            "forced: {forced}: {}",
            events.dump()
        );
        assert_eq!(offers(&events), [Some(wc_offer("wc -l notes.txt")), None]);
        assert_only_host_always(&events);
        host.shutdown();
    }
}

/// No agent option of kind `allow_always` reaches a card: claude-agent-acp's rule-writing and
/// skill options, and one of a kind the host has never seen, are dropped, the host's own
/// `clauseo.always` is offered instead, and a plan exit's options pass unchanged.
#[test]
fn only_the_hosts_always_and_plan_exits_reach_a_card() {
    let dir = Dir::new("always_only_host", &[]);
    let options = json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-always", "name": "Always", "kind": "allow_always"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
        {"optionId": "allow-skill-exact", "name": "Yes, for this skill", "kind": "allow_always"},
        {"optionId": "keep-forever", "name": "Forever", "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ]);
    let mut turn = Vec::new();
    let wc = json!({"command": "wc -l notes.txt"});
    turn.extend(ask("wc", "Bash", &wc, &options, "allow-once"));
    let plan = json!([
        {"optionId": "exit-plan-clear-auto", "name": "Yes, clear context and use auto mode", "kind": "allow_always"},
        {"optionId": "exit-plan-auto", "name": "Yes, and use auto mode", "kind": "allow_always"},
        {"optionId": "exit-plan-default", "name": "Yes, manually approve edits", "kind": "allow_once"},
        {"optionId": "reject", "name": "No, keep planning", "kind": "reject_once"},
    ]);
    turn.push(
        json!({"update": {"update": {"sessionUpdate": "current_mode_update",
        "currentModeId": "plan"}}}),
    );
    turn.push(
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": "t-plan",
        "title": "Ready to code?", "kind": "switch_mode", "status": "pending"}}}),
    );
    turn.push(
        json!({"request": {"id": "plan", "method": "session/request_permission",
        "params": {"sessionId": "sess-1", "toolCall": {"toolCallId": "t-plan",
        "title": "Ready to code?", "kind": "switch_mode", "status": "pending"},
        "options": plan}}}),
    );
    turn.push(json!({"expect_response": {"id": "plan",
        "result": {"outcome": {"outcome": "selected", "optionId": "reject"}}}}));
    let scenario = claude_scenario(&dir, "claude-only-host", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines, then plan.");
    answer_from(&host, &mut events, thread, 0, &["allow-once", "reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let cards: Vec<_> = resolutions(&events)
        .into_iter()
        .map(|(card, _)| card)
        .collect();
    let plan_ids = [
        "exit-plan-clear-auto",
        "exit-plan-auto",
        "exit-plan-default",
        "reject",
    ];
    assert_eq!(
        cards,
        [
            ["allow-once", ALWAYS, "reject"].map(String::from).to_vec(),
            plan_ids.map(String::from).to_vec(),
        ],
        "{}",
        events.dump()
    );
    assert_only_host_always(&events);
    host.shutdown();
}

/// A host with no secret store (Linux, a refused Keychain prompt) keeps nothing: no card offers
/// "Always", answering it is refused, and the same request asks again.
#[test]
fn without_a_secret_store_nothing_is_offered_or_kept() {
    let dir = Dir::new("always_no_secrets", &[]).without_always_secrets();
    let wc = json!({"command": "wc -l notes.txt"});
    let mut turn = Vec::new();
    turn.extend(ask("one", "Bash", &wc, &bash_options(), "allow-once"));
    turn.extend(ask("two", "Bash", &wc, &bash_options(), "allow-once"));
    let scenario = claude_scenario(&dir, "claude-no-secrets", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines twice.");
    asked(&mut events, 1);
    refused_answer(
        &host,
        &events,
        thread,
        0,
        ALWAYS,
        ErrorCode::PermissionUnknownOption,
    );
    answer_from(&host, &mut events, thread, 0, &["allow-once", "allow-once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let card = ["allow-once", "reject"].map(String::from).to_vec();
    let once = Some(("allow-once".to_owned(), PermissionKind::AllowOnce));
    assert_eq!(
        resolutions(&events),
        [(card.clone(), once.clone()), (card, once)],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None, None]);
    assert!(!rules_file(&dir, matter).exists());
    assert_eq!(kept_rules(&host, matter), Vec::<String>::new());
    host.shutdown();
}

/// A rules file the host did not write for this matter holds nothing: one written by hand (an
/// unsigned version 1 file) or one copied from another matter (signed for that matter's id). Its
/// rules answer no request and are not listed; the lawyer's next "Always" sets the file aside as
/// `claude-allow.json.bad-1` and starts a new one.
#[test]
fn a_planted_or_copied_rules_file_keeps_nothing() {
    let rules = ["Bash(wc -l notes.txt)", "Bash(wc -c notes.txt)"];
    let planted = [
        ("hand", json!({"version": 1, "allow": rules}).to_string()),
        (
            "copied",
            signed_rules("01a0c450-6c00-7000-8000-00000000ffff", &rules),
        ),
    ];
    for (how, text) in planted {
        let dir = Dir::new(&format!("always_planted_{how}"), &[]);
        let wc = json!({"command": "wc -l notes.txt"});
        let mut turn = Vec::new();
        turn.extend(ask("one", "Bash", &wc, &bash_options(), "allow-once"));
        let scenario = claude_scenario(&dir, "claude-planted", false, turn);
        dir.set_agents(&[(CLAUDE, scenario)]);
        let host = dir.start(0);
        let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
        let file = rules_file(&dir, matter);
        crate::write::write(&file, text.as_bytes());
        assert_eq!(kept_rules(&host, matter), Vec::<String>::new(), "{how}");
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Count the lines.");
        // The planted rule did not answer: the lawyer is asked.
        answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
        let ended = events.until_turn_ended(turn);
        events.assert_no_mismatch();
        assert_eq!(ended.stop, StopReason::EndTurn, "{how}: {}", events.dump());
        let aside = file.with_file_name("claude-allow.json.bad-1");
        crate::support::wait("the planted file set aside", async {
            while !aside.exists() || !file.exists() {
                async_io::Timer::after(std::time::Duration::from_millis(20)).await;
            }
        });
        assert_eq!(std::fs::read_to_string(&aside).unwrap(), text, "{how}");
        assert_eq!(kept_rules(&host, matter), [rules[0]], "{how}");
        host.shutdown();
    }
}

/// A tool call update and a request `id` about it, of `tool` with `input`, offering
/// `options`, and the answer the agent must get.
fn ask(id: &str, tool: &str, input: &Value, options: &Value, answer: &str) -> [Value; 3] {
    let call = format!("tc-{id}");
    let path = input.get("file_path").cloned();
    let locations: Vec<_> = path.into_iter().map(|path| json!({"path": path})).collect();
    [
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": call,
            "title": tool, "name": tool, "status": "pending", "rawInput": input,
            "locations": locations}}}),
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": {"toolCallId": call, "title": tool,
            "rawInput": input, "locations": locations}, "options": options}}}),
        json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
    ]
}

/// `ChatGPT`'s one-time approval (codex-acp 2.2.2): a command card in the matter folder drops the
/// agent's session and amendment options and offers `clauseo.always`; picked, the agent gets
/// `allow_once` and the host keeps `Run(wc -l notes.txt)`, which answers the next identical card
/// `allow_once` with no card for the lawyer.
#[test]
fn chatgpt_keeps_a_command_and_the_host_answers_it_allow_once() {
    const CODEX: &str = crate::codex::CODEX;
    let dir = Dir::new("always_codex", &[(CODEX, Scenario::Local("codex-always"))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the notes.");
    answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (cards, chosen): (Vec<_>, Vec<_>) = resolutions(&events).into_iter().unzip();
    assert_eq!(
        cards[0],
        ["allow_once", ALWAYS, "decline", "cancel"].map(String::from),
        "{}",
        events.dump()
    );
    assert_eq!(
        chosen,
        [
            Some((ALWAYS.to_owned(), PermissionKind::AllowAlways)),
            Some(("allow_once".to_owned(), PermissionKind::AllowOnce)),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events)[0], Some(wc_offer("wc -l notes.txt")));
    assert_only_host_always(&events);
    until_kept(&dir, matter, 1);
    assert_eq!(kept_rules(&host, matter), ["Run(wc -l notes.txt)"]);
    host.shutdown();
}

/// claude-agent-acp 0.89.1's options for a request `id` about tool call `tc-<id>`, and the
/// answer the agent must get.
#[cfg(unix)]
fn kept_ask(id: &str, tool: &str, input: &Value, answer: &str) -> [Value; 3] {
    ask(id, tool, input, &bash_options(), answer)
}

/// Kept rules answer only the requests Claude Code sends the host, after its checks: neither
/// rule reaches the launch's settings, a kept command into Clauseo's data folder is still
/// refused by the data fence (unseen), and a kept `Edit(*)` does not answer an edit of a file
/// whose folder in the matter is a link to another folder: it reaches the lawyer.
#[cfg(unix)]
#[test]
fn a_kept_rule_never_passes_the_fence_or_a_swapped_link() {
    let dir = Dir::new("always_checks", &[]);
    let data = dir.data().to_owned();
    crate::write::write(&data.join("update.json"), b"{}\n");
    let folder = dir.folder("m", &[("a.txt", "x\n")]);
    let other = dir.folder("other", &[("out.txt", "x\n")]);
    crate::write::symlink(&other, &folder.join("work"));
    let cat = format!("cat {}/update.json", data.display());
    let out = format!("{}/work/out.txt", folder.display());
    let kept = [format!("Bash({cat})"), "Edit(*)".to_owned()];
    let excludes = serde_json::json!(["{{excludes}}", kept[0], kept[1]]);
    let mut lines = vec![
        serde_json::json!({"scenario": "claude-always-checks", "description": "Kept rules reach \
            no settings; the fence and the real path still decide", "on_stdin_close": "exit"}),
        serde_json::json!({"expect": {"method": "initialize", "as": "init",
            "params": {"protocolVersion": 1}}}),
        serde_json::json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        serde_json::json!({"expect": {"method": "session/new", "as": "new", "params": {
            "cwd": "{{any}}", "_meta": {"claudeCode": {"options": {"settings": {
            "permissions": {"allow": excludes}}}}}}}}),
        serde_json::json!({"respond": {"to": "new", "result": {"sessionId": "sess-1",
            "modes": {"currentModeId": "auto", "availableModes": [
            {"id": "default", "name": "Manual"}, {"id": "auto", "name": "Auto"}]}}}}),
        serde_json::json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(kept_ask(
        "fence",
        "Bash",
        &serde_json::json!({"command": cat}),
        "reject",
    ));
    let edit = serde_json::json!({"file_path": out, "old_string": "x", "new_string": "y"});
    lines.extend(kept_ask("link", "Edit", &edit, "reject"));
    lines.push(serde_json::json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let mut text = String::new();
    for line in &lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    let file = folder.with_file_name("claude-always-checks.jsonl");
    crate::write::write(&file, text.as_bytes());
    dir.set_agents(&[(CLAUDE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    let kept: Vec<&str> = kept.iter().map(String::as_str).collect();
    keep(&dir, matter, &kept);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the update and edit the file.");
    answer_from(&host, &mut events, thread, 0, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Only the Edit reached the lawyer: the fence refused the command unseen.
    let chosen = resolutions(&events);
    assert_eq!(chosen.len(), 1, "{}", events.dump());
    assert_eq!(
        chosen[0].1,
        Some(("reject".to_owned(), PermissionKind::RejectOnce)),
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A kept `Edit(*)` answers a change to a file in the matter, but never one reached through a
/// link out of it: an Edit or a Write of `<matter>/x/out.txt`, where `x` links to a folder
/// outside, and an Edit of `<matter>/linked.txt`, a link to a file outside, each reach the
/// lawyer on a card that offers no "Always".
#[cfg(unix)]
#[test]
fn a_kept_edit_all_never_follows_a_link_out_of_the_matter() {
    let dir = Dir::new("always_edit_links", &[]);
    let folder = dir.folder("m", &[("a.txt", "x\n")]);
    let scratch = dir.folder("scratch", &[("out.txt", "x\n"), ("target.txt", "x\n")]);
    crate::write::symlink(&scratch, &folder.join("x"));
    crate::write::symlink(&scratch.join("target.txt"), &folder.join("linked.txt"));
    let root = folder.display();
    let change = |path: String| json!({"file_path": path, "old_string": "x", "new_string": "y"});
    let write = json!({"file_path": format!("{root}/x/out.txt"), "content": "y"});
    let mut turn = Vec::new();
    let inside = change(format!("{root}/a.txt"));
    turn.extend(ask(
        "inside",
        "Edit",
        &inside,
        &bash_options(),
        "allow-once",
    ));
    let through = change(format!("{root}/x/out.txt"));
    turn.extend(ask(
        "folder-edit",
        "Edit",
        &through,
        &bash_options(),
        "reject",
    ));
    turn.extend(ask(
        "folder-write",
        "Write",
        &write,
        &bash_options(),
        "reject",
    ));
    let linked = change(format!("{root}/linked.txt"));
    turn.extend(ask("file-edit", "Edit", &linked, &bash_options(), "reject"));
    let scenario = claude_scenario(&dir, "claude-edit-links", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    keep(&dir, matter, &["Edit(*)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the files.");
    answer_from(
        &host,
        &mut events,
        thread,
        1,
        &["reject", "reject", "reject"],
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let card = ["allow-once", "reject"].map(String::from).to_vec();
    let reject = Some(("reject".to_owned(), PermissionKind::RejectOnce));
    assert_eq!(
        resolutions(&events),
        [
            // The file in the matter: the host's, from the kept rule.
            (
                card.clone(),
                Some(("allow-once".to_owned(), PermissionKind::AllowOnce))
            ),
            (card.clone(), reject.clone()),
            (card.clone(), reject.clone()),
            (card, reject),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None, None, None, None]);
    host.shutdown();
}
