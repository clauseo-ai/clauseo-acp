//! "Always for this matter" on a site (decision record 0063, `agent/always.rs` `claude_rule`): a
//! Claude `SandboxNetworkAccess` card for a host keeps the same rule as a `WebFetch` of that site,
//! `WebFetch(domain:<host>)`, so one kept entry answers both. The card offers the host's
//! `clauseo.always` and no agent "don't ask again"; a host the parser refuses gets no "Always".

use clauseo_protocol::{
    AlwaysKind, AlwaysOffer, CommandOutcome, DomainEvent, ErrorCode, HostCommand, PermissionKind,
    StopReason, StreamId,
};
use serde_json::{Value, json};

use crate::always::{
    ALWAYS, CLAUDE, ask, asked, assert_only_host_always, claude_scenario, keep, kept_rules, offers,
    refused_answer, resolutions, rules_file, until_kept,
};
use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, Scenario, matter_with_thread};

/// The rule a kept site is: a `WebFetch` of its domain, for both tools.
const SITE_RULE: &str = "WebFetch(domain:sci.gov.in)";

/// claude-agent-acp 0.89.1's options on a `WebFetch` or a sandbox network card: an allow-once, an
/// agent's `allow_always` "don't ask again", and a reject.
fn network_options() -> Value {
    json!([
        {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
        {"optionId": "allow-with-updates", "name": "Yes, and don't ask again for this site",
         "kind": "allow_always"},
        {"optionId": "reject", "name": "No", "kind": "reject_once"},
    ])
}

/// A request `id` for a `WebFetch` of `url`, answered `answer` (what the agent must get).
fn fetch(id: &str, url: &str, answer: &str) -> [Value; 3] {
    ask(
        id,
        "WebFetch",
        &json!({"url": url}),
        &network_options(),
        answer,
    )
}

/// A request `id` for Claude's shell asking for network access to `host`, answered `answer`.
fn sandbox(id: &str, host: &str, answer: &str) -> [Value; 3] {
    ask(
        id,
        "SandboxNetworkAccess",
        &json!({"host": host}),
        &network_options(),
        answer,
    )
}

/// The offer a site card carries for `host`.
fn site_offer(host: &str) -> AlwaysOffer {
    AlwaysOffer {
        kind: AlwaysKind::Site,
        shown: Some(host.to_owned()),
        cwd: None,
    }
}

/// The options a card lists, by id and kind, of the request at `index` (0-based).
fn listed(events: &Events, index: usize) -> Vec<(String, PermissionKind)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(
                asked
                    .options
                    .iter()
                    .map(|o| (o.option_id.to_string(), o.kind))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .nth(index)
        .unwrap_or_else(|| panic!("no request {index}: {}", events.dump()))
}

/// The host's answer to a card the host answers itself: `allow-once`.
fn host_once() -> (String, PermissionKind) {
    ("allow-once".to_owned(), PermissionKind::AllowOnce)
}

/// The card a site card lists when the host offers "Always": allow-once, the host's
/// `clauseo.always`, reject.
fn site_card() -> Vec<String> {
    ["allow-once", ALWAYS, "reject"].map(String::from).to_vec()
}

/// The card a site card lists when the host offers no "Always": allow-once and reject.
fn plain_card() -> Vec<String> {
    ["allow-once", "reject"].map(String::from).to_vec()
}

/// Keeping from a `WebFetch` card (`https://sci.gov.in/a`, "Always"), then a sandbox card for
/// `sci.gov.in` in the same turn: the host answers it `allow-once`, and the lawyer gets no card.
#[test]
fn a_site_kept_from_a_fetch_answers_a_sandbox_card() {
    let dir = Dir::new("sites_fetch_then_sandbox", &[]);
    let mut turn = Vec::new();
    turn.extend(fetch("fetch", "https://sci.gov.in/a", "allow-once"));
    turn.extend(sandbox("shell", "sci.gov.in", "allow-once"));
    let scenario = claude_scenario(&dir, "claude-sites-fetch-shell", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Fetch the page, then run the tests.");
    answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [
            (
                site_card(),
                Some((ALWAYS.to_owned(), PermissionKind::AllowAlways))
            ),
            (plain_card(), Some(host_once())),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [Some(site_offer("sci.gov.in")), None]);
    assert_only_host_always(&events);
    until_kept(&dir, matter, 1);
    assert_eq!(kept_rules(&host, matter), [SITE_RULE]);
    host.shutdown();
}

/// The reverse: keeping from a sandbox card for `sci.gov.in`, then a `WebFetch` of
/// `https://sci.gov.in/b`, which the host answers `allow-once` with no card for the lawyer.
#[test]
fn a_site_kept_from_a_sandbox_card_answers_a_fetch() {
    let dir = Dir::new("sites_sandbox_then_fetch", &[]);
    let mut turn = Vec::new();
    turn.extend(sandbox("shell", "sci.gov.in", "allow-once"));
    turn.extend(fetch("fetch", "https://sci.gov.in/b", "allow-once"));
    let scenario = claude_scenario(&dir, "claude-sites-shell-fetch", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests, then fetch the page.");
    answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [
            (
                site_card(),
                Some((ALWAYS.to_owned(), PermissionKind::AllowAlways))
            ),
            (plain_card(), Some(host_once())),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [Some(site_offer("sci.gov.in")), None]);
    assert_only_host_always(&events);
    until_kept(&dir, matter, 1);
    assert_eq!(kept_rules(&host, matter), [SITE_RULE]);
    host.shutdown();
}

/// A sandbox card for `sci.gov.in` lists the host's `clauseo.always` as an `AllowAlways` and
/// no agent option of that kind: the agent's "don't ask again" is dropped, and the card offers
/// the site as `Site` with `shown` `sci.gov.in`.
#[test]
fn a_sandbox_card_offers_the_hosts_always_for_the_site() {
    let dir = Dir::new("sites_sandbox_card", &[]);
    let turn = sandbox("shell", "sci.gov.in", "allow-once").to_vec();
    let scenario = claude_scenario(&dir, "claude-sites-card", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    answer_from(&host, &mut events, thread, 0, &[ALWAYS]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let always = (ALWAYS.to_owned(), PermissionKind::AllowAlways);
    assert_eq!(
        listed(&events, 0),
        [
            ("allow-once".to_owned(), PermissionKind::AllowOnce),
            always.clone(),
            ("reject".to_owned(), PermissionKind::RejectOnce),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(
        resolutions(&events),
        [(site_card(), Some(always))],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [Some(site_offer("sci.gov.in"))]);
    assert_only_host_always(&events);
    until_kept(&dir, matter, 1);
    assert_eq!(kept_rules(&host, matter), [SITE_RULE]);
    host.shutdown();
}

/// A kept site is removed from the matter, and the running agent with it: the next sandbox card
/// for `sci.gov.in` reaches the lawyer, who rejects it. The first card, before the removal, is
/// answered `allow-once` by the host from the kept rule.
#[test]
fn a_removed_site_is_asked_again_for_the_shell() {
    let dir = Dir::new(
        "sites_remove",
        &[(CLAUDE, Scenario::Local("claude-sites-remove"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    keep(&dir, matter, &[SITE_RULE]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let remove = HostCommand::RemoveKeptPermission {
        matter,
        rule: SITE_RULE.to_owned(),
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    assert_eq!(kept_rules(&host, matter), Vec::<String>::new());
    let (turn, _) = host.prompt(thread, "Run them again.");
    answer_from(&host, &mut events, thread, 1, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        resolutions(&events),
        [
            (plain_card(), Some(host_once())),
            (
                site_card(),
                Some(("reject".to_owned(), PermissionKind::RejectOnce))
            ),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None, Some(site_offer("sci.gov.in"))]);
    host.shutdown();
}

/// A host the parser refuses (`sci.gov.in:443`, a port) gets a card with no "Always": answering
/// the host's id is refused, the agent's allow-once is answered, and nothing is kept.
#[test]
fn a_host_the_parser_refuses_gets_no_always() {
    let dir = Dir::new("sites_refused_host", &[]);
    let turn = sandbox("shell", "sci.gov.in:443", "allow-once").to_vec();
    let scenario = claude_scenario(&dir, "claude-sites-refused", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    asked(&mut events, 1);
    refused_answer(
        &host,
        &events,
        thread,
        0,
        ALWAYS,
        ErrorCode::PermissionUnknownOption,
    );
    answer_from(&host, &mut events, thread, 0, &["allow-once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        listed(&events, 0),
        [
            ("allow-once".to_owned(), PermissionKind::AllowOnce),
            ("reject".to_owned(), PermissionKind::RejectOnce),
        ],
        "{}",
        events.dump()
    );
    assert_eq!(
        resolutions(&events),
        [(plain_card(), Some(host_once()))],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None]);
    assert_only_host_always(&events);
    host.settle();
    assert_eq!(kept_rules(&host, matter), Vec::<String>::new());
    assert!(!rules_file(&dir, matter).exists());
    host.shutdown();
}
