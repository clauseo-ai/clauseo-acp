//! The header's `protocol`, and what an app-server scenario's load checks drop and add.

use super::*;

const APP_HEADER: &str = r#"{"scenario":"t","description":"test","protocol":"app-server"}"#;

/// Scenario `t` as an app-server scenario with `lines` after its header.
fn app_server(lines: &[&str]) -> Result<Scenario, Error> {
    let text = std::iter::once(APP_HEADER)
        .chain(lines.iter().copied())
        .collect::<Vec<_>>()
        .join("\n");
    parse("t", &text, &Includes::Memory(&[]))
}

fn app_error(lines: &[&str]) -> String {
    match app_server(lines) {
        Ok(_) => panic!("loaded: {lines:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn protocol_is_acp_unless_the_header_says_app_server() {
    let protocol = |header: &str| parse("t", header, &Includes::Memory(&[])).map(|s| s.protocol);
    assert_eq!(protocol(HEADER).unwrap(), Protocol::Acp);
    let acp = r#"{"scenario":"t","description":"d","protocol":"acp"}"#;
    assert_eq!(protocol(acp).unwrap(), Protocol::Acp);
    assert_eq!(protocol(APP_HEADER).unwrap(), Protocol::AppServer);
    let bad = r#"{"scenario":"t","description":"d","protocol":"app_server"}"#;
    assert_eq!(
        protocol(bad).unwrap_err().to_string(),
        "t.jsonl:1: header: unknown variant `app_server`, expected `acp` or `app-server`"
    );
}

#[test]
fn an_app_server_expect_is_a_notification_exactly_when_it_has_no_label() {
    let initialized = r#"{"expect":{"method":"initialized"}}"#;
    assert!(app_server(&[initialized]).is_ok());
    assert_eq!(
        load_error(&[initialized], &[]),
        "t.jsonl:2: expect `initialized`: a request needs an `as` label"
    );
    let labelled_cancel = r#"{"expect":{"method":"session/cancel","as":"c"}}"#;
    assert!(app_server(&[labelled_cancel]).is_ok());
    let all = r#"{"all":[{"expect":{"method":"initialized"}},{"expect":{"method":"x","as":"x"}}]}"#;
    assert!(app_server(&[all]).is_ok());
    let branch = r#"{"one_of":[[{"expect":{"method":"initialized"}}]]}"#;
    assert!(app_server(&[branch]).is_ok());
}

#[test]
fn app_server_messages_skip_the_sdk_check() {
    let read = r#"{"expect":{"method":"account/read","as":"a"}}"#;
    let answer = r#"{"respond":{"to":"a","result":{"account":null,"requiresOpenaiAuth":true}}}"#;
    let notify = r#"{"notify":{"method":"account/rateLimits/updated","params":{"x":1}}}"#;
    let request = r#"{"request":{"id":"s1","method":"item/tool/call","params":{}}}"#;
    assert!(app_server(&[read, answer, notify, request]).is_ok());
    let init = r#"{"expect":{"method":"initialize","as":"a"}}"#;
    let bad_init = r#"{"respond":{"to":"a","result":{"protocolVersion":"one"}}}"#;
    assert!(app_server(&[init, bad_init]).is_ok());
    assert!(
        load_error(&[notify], &[]).contains("mark the step \"raw\": true"),
        "an ACP scenario still checks"
    );
}

#[test]
fn raw_and_update_are_refused_in_an_app_server_scenario() {
    let raw = r#"{"notify":{"method":"x","raw":true}}"#;
    let refused = "has no use in an app-server scenario";
    assert!(
        app_error(&[raw]).starts_with("t.jsonl:2: \"raw\": true"),
        "{}",
        app_error(&[raw])
    );
    assert!(app_error(&[raw]).contains(refused));
    let in_branch = format!(r#"{{"one_of":[[{{"expect":{{"method":"x"}}}},{raw}]]}}"#);
    assert!(app_error(&[&in_branch]).contains(refused));
    let update = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x"}}}}"#;
    assert!(app_error(&[update]).contains("update sends ACP's session/update"));
    assert!(parse_text(&[raw], &[]).is_ok(), "raw stays allowed in ACP");
}
