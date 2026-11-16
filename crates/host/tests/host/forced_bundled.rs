//! Forced ask's bundled tools (`actor/turn/asks/bundled.rs`, decision record 0063): in a
//! forced-ask Claude session the host answers Clauseo's own case-law search and read tools
//! `allow_once` with no card, only when the host itself sent its `case-law` entry; `recharge`
//! asks, and so does the same tool name with no entry of the host's (a lawyer's server named
//! `case-law` is dropped, never trusted by its name). A normal task (three reads, one case-law
//! search, one edit) raises one card at most: the edit's. Each scenario checks the option the
//! agent got, so a request the host answered that the lawyer should have seen (or the other way
//! round) is a mismatch.

use clauseo_host::HostConfig;
use clauseo_protocol::{DomainEvent, FolderSupportKind, StopReason, StreamId};
use serde_json::json;

use crate::case_law::{Connector, wire};
use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";

/// The guard program the forced-ask host names, absolute on every system; the fake agent never
/// runs it.
fn guard() -> std::path::PathBuf {
    std::env::temp_dir().join("clauseo-desktop")
}

/// The config of a host whose launches' folder check answers `network` (forced ask).
fn forced(config: HostConfig) -> HostConfig {
    config
        .with_folder_support(Some(FolderSupportKind::Network))
        .with_pre_write_guard(guard())
}

/// Every logged permission request's option ids, in order: the cards the lawyer saw.
fn cards(events: &Events) -> Vec<Vec<String>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(
                asked
                    .options
                    .iter()
                    .map(|option| option.option_id.to_string())
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// A forced-ask host with Clauseo's case-law entry sent (the connector signed in, its relay
/// open) and a Claude thread playing `scenario` in a matter holding `files`.
fn with_case_law(
    test: &str,
    scenario: &'static str,
    connector: &Connector,
    files: &[(&str, &str)],
) -> (Dir, Running, clauseo_protocol::ThreadId) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start_with(0, |config| wire(forced(config), Some(&connector.relays)));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", files, CLAUDE);
    (dir, host, thread)
}

/// `claude-forced-bundled`: `search_law`, `read_document` and `account` are answered by the host
/// with nothing logged; `recharge`, which opens a payment, is the one card.
#[test]
fn forced_ask_runs_case_laws_own_tools_unasked_but_recharge_asks() {
    let connector = Connector::new(true);
    let (_dir, host, thread) = with_case_law(
        "forced_bundled",
        "claude-forced-bundled",
        &connector,
        &[("brief.txt", "x\n")],
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Forced ask drops the agent's rule-writing option from the one card.
    assert_eq!(
        cards(&events),
        [vec!["allow-once".to_owned(), "reject".to_owned()]],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// `claude-forced-bundled-unsent`: with no relay the host sends no `case-law` entry, and the
/// lawyer's own server named `case-law` is dropped as reserved; the same `search_law` tool name
/// then asks, because only the host's own entry counts, never the name.
#[test]
fn forced_ask_asks_for_a_case_law_tool_the_host_did_not_send() {
    let dir = Dir::new("forced_bundled_unsent", &[]);
    let bin = dir.folder("bin", &[]);
    let lawyers = json!({"mcp": {"servers": {"case-law": {
        "command": bin.join("case-law").display().to_string(), "enabled": true}}}});
    dir.set_settings(
        &[(CLAUDE, Scenario::Local("claude-forced-bundled-unsent"))],
        lawyers,
    );
    let host = dir.start_with(0, |config| wire(forced(config), None));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &["allow-once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(cards(&events).len(), 1, "{}", events.dump());
    host.shutdown();
}

const TASK_FILES: [(&str, &str); 4] = [
    ("a.txt", "a\n"),
    ("b.txt", "b\n"),
    ("c.txt", "c\n"),
    ("brief.txt", "x\n"),
];

/// The PR's "one card" row: a normal task in a forced-ask matter (read three papers, search case
/// law, edit one file) raises one card, the edit's; the reads never ask and the host answers the
/// search.
#[test]
fn a_normal_forced_ask_task_raises_one_card() {
    let connector = Connector::new(true);
    let (_dir, host, thread) = with_case_law(
        "forced_one_card",
        "claude-forced-one-card",
        &connector,
        &TASK_FILES,
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers, find the law, fix the brief.");
    answer_from(&host, &mut events, thread, 0, &["allow-once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(cards(&events).len(), 1, "{}", events.dump());
    host.shutdown();
}

/// Once the host's option rewrite offers `clauseo.always` (the core of decision record 0063),
/// forced ask keeps it: the one card of the normal task offers "Always allow changes to files in
/// this matter", and answering it reaches the agent as its `allow-once`.
#[test]
fn the_one_card_offers_always_for_this_matter_in_forced_ask() {
    let connector = Connector::new(true);
    let (_dir, host, thread) = with_case_law(
        "forced_one_card_always",
        "claude-forced-one-card",
        &connector,
        &TASK_FILES,
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers, find the law, fix the brief.");
    crate::permissions::until_first_request(&mut events);
    host.settle();
    events.drain_ready();
    let cards = cards(&events);
    assert_eq!(cards.len(), 1, "{}", events.dump());
    assert!(
        cards[0].iter().any(|id| id == "clauseo.always"),
        "{}",
        events.dump()
    );
    assert!(
        !cards[0].iter().any(|id| id == "allow-with-updates"),
        "{}",
        events.dump()
    );
    answer_from(&host, &mut events, thread, 0, &["clauseo.always"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}
