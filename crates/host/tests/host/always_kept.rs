//! "Always for this matter" (decision record 0063) beyond #1109's tests in `always.rs`: an
//! agent's own host option ids and rule-writing options never reach a card, a host with no secret
//! store or a planted rules file keeps nothing, `ChatGPT` keeps a command, a kept `Edit(*)` never
//! follows a link out of the matter, and kept rules survive a restart, stay in their matter and
//! are deleted with it.

use clauseo_protocol::{
    ErrorCode, FolderSupportKind, HostCommand, MatterId, PermissionKind, StopReason, StreamId,
    ThreadId,
};
use serde_json::json;

use crate::always::{
    ALWAYS, CLAUDE, ask, asked, assert_only_host_always, bash_options, claude_scenario, guard,
    keep, kept_rules, offers, refused_answer, resolutions, rules_file, signed_rules, until_kept,
    wc_offer,
};
use crate::permissions::answer_from;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

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

/// One Claude prompt asking once to run `wc -l notes.txt`, answered `allow-once` (the lawyer's
/// `clauseo.always` reaches the agent so too), as scenario `name` of `dir`.
fn one_wc(dir: &Dir, name: &str) -> Scenario {
    let wc = json!({"command": "wc -l notes.txt"});
    let turn = ask("wc", "Bash", &wc, &bash_options(), "allow-once").to_vec();
    claude_scenario(dir, name, false, turn)
}

/// Keeps `Bash(wc -l notes.txt)` for `thread`'s matter through the lawyer's `clauseo.always`,
/// and waits for the rules file to hold it.
fn keep_wc(dir: &Dir, host: &Running, matter: MatterId, thread: ThreadId) {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines.");
    answer_from(host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    until_kept(dir, matter, 1);
}

/// A rule kept through "Always" outlives the host: after a restart on the same data folder (and
/// the same key in the secret store), a new thread of the matter gets the same request answered
/// `allow-once` by the host, with no card for the lawyer.
#[test]
fn a_kept_rule_survives_a_restart() {
    let dir = Dir::new("always_restart", &[]);
    dir.set_agents(&[(CLAUDE, one_wc(&dir, "claude-one-wc"))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    keep_wc(&dir, &host, matter, thread);
    host.shutdown();

    let host = dir.start(1);
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count them again.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [(
            ["allow-once", "reject"].map(String::from).to_vec(),
            Some(("allow-once".to_owned(), PermissionKind::AllowOnce))
        )],
        "{}",
        events.dump()
    );
    assert_eq!(kept_rules(&host, matter), ["Bash(wc -l notes.txt)"]);
    host.shutdown();
}

/// A rule answers only its own matter's threads: a thread of another matter gets the identical
/// request as a card for the lawyer, still offering "Always", and answering it once changes
/// neither matter's list.
#[test]
fn a_kept_rule_stays_in_its_matter() {
    let dir = Dir::new("always_own_matter", &[]);
    dir.set_agents(&[(CLAUDE, one_wc(&dir, "claude-one-wc"))]);
    let host = dir.start(0);
    let (a, thread_a, _) = matter_with_thread(&dir, &host, "a", &[], CLAUDE);
    keep_wc(&dir, &host, a, thread_a);

    let (b, thread_b, _) = matter_with_thread(&dir, &host, "b", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread_b));
    let (turn, _) = host.prompt(thread_b, "Count the lines.");
    answer_from(&host, &mut events, thread_b, 0, &["allow-once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [(
            ["allow-once", ALWAYS, "reject"].map(String::from).to_vec(),
            Some(("allow-once".to_owned(), PermissionKind::AllowOnce))
        )],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [Some(wc_offer("wc -l notes.txt"))]);
    host.settle();
    assert_eq!(kept_rules(&host, a), ["Bash(wc -l notes.txt)"]);
    assert_eq!(kept_rules(&host, b), Vec::<String>::new());
    assert!(!rules_file(&dir, b).exists());
    host.shutdown();
}

/// Forgetting a matter deletes its kept rules file.
#[test]
fn a_forgotten_matter_takes_its_kept_rules_with_it() {
    let dir = Dir::new("always_forget", &[]);
    dir.set_agents(&[(CLAUDE, one_wc(&dir, "claude-one-wc"))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    keep_wc(&dir, &host, matter, thread);
    host.settle();
    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter }),
        clauseo_protocol::CommandOutcome::Done
    );
    let file = rules_file(&dir, matter);
    crate::support::wait("the kept rules file gone", async {
        while file.exists() {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
    host.shutdown();
}
