//! "Always for this matter" on a Claude request (route A, `agent/always.rs`): the lawyer's
//! `allow_always` answer reaches claude-agent-acp as `allow_once`, so it writes nothing into the
//! matter, and the host keeps the rule beside the matter's notes instead; a later request with
//! the same rule is allowed by the host, logged, in this session and after a relaunch. No launch
//! carries a kept rule in its flag settings: Claude Code asks the host, which answers only after
//! its data fence and held-open checks. Forced ask keeps and applies nothing. Each scenario checks
//! the option the agent got and the `_meta` it was launched with.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, FolderSupportKind, HostCommand, MatterId,
    PermissionChoice, PermissionKind, Principal, StopReason, StreamId,
};

use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";

/// The guard program the forced-ask host names, absolute on every system; the fake agent never
/// runs it.
fn guard() -> std::path::PathBuf {
    std::env::temp_dir().join("clauseo-desktop")
}

/// What the lawyer's own `.claude` folder in the matter holds; it must stay as it is.
const OWN_SETTINGS: &str = "{\"permissions\": {\"allow\": [\"Read\"]}}\n";

/// `<data>/agent-state/<matter>/claude-allow.json`.
fn rules_file(dir: &Dir, matter: MatterId) -> std::path::PathBuf {
    dir.data()
        .join("agent-state")
        .join(matter.to_string())
        .join("claude-allow.json")
}

/// A request's option ids and its chosen option, if it was resolved.
type Resolution = (Vec<String>, Option<(String, PermissionKind)>);

/// Each request's [`Resolution`], in order; every resolution is the desktop's.
fn resolutions(events: &Events) -> Vec<Resolution> {
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
    answer_from(&host, &mut events, thread, 0, &["allow-with-updates"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let all = ["allow-once", "allow-with-updates", "reject"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        resolutions(&events),
        [
            // The "Always" option is offered, and the log keeps the lawyer's choice.
            (
                all.clone(),
                Some(("allow-with-updates".to_owned(), PermissionKind::AllowAlways))
            ),
            // The second is the host's, from the kept rule.
            (
                all,
                Some(("allow-once".to_owned(), PermissionKind::AllowOnce))
            ),
        ],
        "{}",
        events.dump()
    );
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

/// Forced ask: a rule the matter kept earlier is neither in the launch's settings nor used to
/// answer, and the request offers no "Always" option to keep another.
#[test]
fn forced_ask_keeps_and_applies_nothing() {
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
    let file = rules_file(&dir, matter);
    let kept = "{\"version\": 1, \"allow\": [\"Bash(wc -l notes.txt)\"]}\n";
    crate::write::write(&file, kept.as_bytes());
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    events.until("permission request 1", |e| {
        !e.permission_requests().is_empty()
    });
    host.settle();
    events.drain_ready();
    assert_eq!(
        resolutions(&events),
        [(vec!["allow-once".to_owned(), "reject".to_owned()], None)],
        "{}",
        events.dump()
    );
    let answer = HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[0],
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    assert_eq!(host.ok(answer), CommandOutcome::Done);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), kept);
    assert!(!folder.join(".claude").exists());
    host.shutdown();
}

/// A kept rule never reaches outside the matter folder, so it never widens past the data fence
/// (`agent::always::within`): a path rule the file holds for a sibling folder, or for the
/// matter's `.clauseo`, is dropped at the launch, and the lawyer's "Always" on a file outside the
/// matter keeps nothing. A file inside the matter is kept and answers its next request.
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
        format!("Edit(/{root}-b/a.txt)"),
        format!("Edit(/{root}/.clauseo/matter.json)"),
    ];
    let file = rules_file(&dir, matter);
    let kept = serde_json::json!({"version": 1, "allow": ["Bash(wc -l notes.txt)", outside[0], outside[1]]});
    crate::write::write(&file, kept.to_string().as_bytes());
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the files.");
    let answers = ["allow-with-updates", "allow-with-updates", "reject"];
    let asked = |events: &mut Events, n: usize| {
        events.until(&format!("permission request {n}"), |e| {
            e.permission_requests().len() >= n
        });
    };
    for (n, option) in [(1, answers[0]), (2, answers[1]), (4, answers[2])] {
        asked(&mut events, n);
        let request = events.permission_requests()[n - 1];
        let answer = HostCommand::AnswerPermission {
            thread,
            request,
            option_id: AgentChoiceId::new(option).unwrap(),
        };
        assert_eq!(host.ok(answer), CommandOutcome::Done, "{}", events.dump());
    }
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let chosen: Vec<_> = resolutions(&events)
        .into_iter()
        .map(|(_, chosen)| chosen.map(|(option, _)| option))
        .collect();
    let want = [
        "allow-with-updates",
        "allow-with-updates",
        "allow-once",
        "reject",
    ];
    assert_eq!(
        chosen,
        want.map(|o| Some(o.to_owned())),
        "{}",
        events.dump()
    );
    let inside = format!("Edit(/{root}/brief.txt)");
    let has = |rule: &str| std::fs::read_to_string(&file).is_ok_and(|text| text.contains(rule));
    crate::support::wait("the kept rule", async {
        while !has(&inside) {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
    // The rules read from the file stay in it; nothing outside the matter was added.
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("-b/a.txt").count(), 1, "{text}");
    assert!(!folder.join(".claude").exists());
    host.shutdown();
}

/// claude-agent-acp 0.89.1's options for a request `id` about tool call `call`, and the answer
/// the agent must get.
#[cfg(unix)]
fn kept_ask(
    id: &str,
    tool: &str,
    input: &serde_json::Value,
    answer: &str,
) -> [serde_json::Value; 3] {
    let call = format!("tc-{id}");
    let path = input.get("file_path").cloned();
    let locations: Vec<_> = path
        .into_iter()
        .map(|path| serde_json::json!({"path": path}))
        .collect();
    let options = serde_json::json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again", "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ]);
    [
        serde_json::json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": call,
            "title": tool, "name": tool, "status": "pending", "rawInput": input,
            "locations": locations}}}),
        serde_json::json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": {"toolCallId": call, "title": tool,
            "rawInput": input, "locations": locations}, "options": options}}}),
        serde_json::json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
    ]
}

/// Kept rules answer only the requests Claude Code sends the host, after its checks: neither
/// rule reaches the launch's settings, a kept command into Clauseo's data folder is still
/// refused by the data fence (unseen), and a kept Edit whose folder was swapped for a link to
/// another folder after it was kept reaches the lawyer instead of being allowed.
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
    let kept = [format!("Bash({cat})"), format!("Edit(/{out})")];
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
    let rules = serde_json::json!({"version": 1, "allow": kept});
    crate::write::write(&rules_file(&dir, matter), rules.to_string().as_bytes());
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
