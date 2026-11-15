//! "Always for this matter" on a Claude request (route A, `agent/always.rs`): the lawyer's
//! `allow_always` answer reaches claude-agent-acp as `allow_once`, so it writes nothing into the
//! matter, and the host keeps the rule beside the matter's notes instead; a later request with
//! the same rule is allowed by the host, logged, and a later launch carries the rule in its flag
//! settings. Forced ask keeps and applies nothing. Each scenario checks the option the agent got
//! and the `_meta` it was launched with.

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
    let kept = || std::fs::read_to_string(&file).is_ok_and(|text| text.contains("Bash(npm test)"));
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

    // The next launch carries the rule, and the host answers the request from it again.
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
    let kept = "{\"version\": 1, \"allow\": [\"Bash(npm test)\"]}\n";
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
    let kept =
        serde_json::json!({"version": 1, "allow": ["Bash(npm test)", outside[0], outside[1]]});
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
