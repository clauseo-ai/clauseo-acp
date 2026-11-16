//! "Always for this matter" (decision record 0063, `agent/always.rs`): no agent option that
//! would write a rule reaches the lawyer; the host offers its own `clauseo.always` when the
//! request gives a rule it can keep, and the agent gets the request's allow-once option, so no
//! agent writes anything into the matter. The host keeps the rule beside the matter's notes,
//! signed with a key from the secret store, and a later request with the same rule is allowed by
//! the host, logged, in this session and after a relaunch, in forced ask too. No launch carries a
//! kept rule in its flag settings: the agent asks the host, which answers only after its data
//! fence and held-open checks. Each scenario checks the option the agent got. The helpers here
//! serve `always_kept.rs` too, which has the rest: dropped options, no secret store, planted
//! files, `ChatGPT`, links, restarts, other matters and forgetting.

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

pub(crate) const CLAUDE: &str = "claude-acp";

/// The host's "Always for this matter" option.
pub(crate) const ALWAYS: &str = "clauseo.always";

/// The guard program the forced-ask host names, absolute on every system; the fake agent never
/// runs it.
pub(crate) fn guard() -> std::path::PathBuf {
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
pub(crate) fn refused_answer(
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
pub(crate) fn asked(events: &mut Events, n: usize) {
    events.until(&format!("permission request {n}"), |e| {
        e.permission_requests().len() >= n
    });
}

/// The card's offer for Claude's `wc -l notes.txt` in the matter folder.
pub(crate) fn wc_offer(command: &str) -> AlwaysOffer {
    AlwaysOffer {
        kind: AlwaysKind::Command,
        shown: Some(command.to_owned()),
        cwd: None,
    }
}

/// claude-agent-acp 0.89.1's options on a Bash call.
pub(crate) fn bash_options() -> Value {
    json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ])
}

/// A Claude scenario named `name`, written into `dir`'s work folder: a claude-acp handshake
/// (in default with no auto mode when `forced`, as a forced-ask launch is; else in auto), one
/// prompt whose steps are `turn`, then `Done.`.
pub(crate) fn claude_scenario(dir: &Dir, name: &str, forced: bool, turn: Vec<Value>) -> Scenario {
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

/// A tool call update and a request `id` about it, of `tool` with `input`, offering
/// `options`, and the answer the agent must get.
pub(crate) fn ask(
    id: &str,
    tool: &str,
    input: &Value,
    options: &Value,
    answer: &str,
) -> [Value; 3] {
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
