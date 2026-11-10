//! claude-agent-acp 0.89.1's real answers (`fixtures/claude-agent-acp-0.89.1.jsonl`, captured
//! on Linux with a scratch, signed-out Claude home: its `initialize` with the new capability
//! fields, its `_auth/status_update`, its `session/new` with modes and config options, its first
//! `available_commands_update`, its `session/prompt` refusal, and its `session/new` refusal of a
//! folder it cannot stat, the path made `/matters/lease`).

use super::*;

const FIXTURE: &str = include_str!("fixtures/claude-agent-acp-0.89.1.jsonl");

/// The fixture's line `n` (0-based).
fn line(n: usize) -> Value {
    let line = FIXTURE.lines().nth(n).expect("a fixture line");
    serde_json::from_str(line).unwrap()
}

/// The fixture's answer `n`, with the id of `request`.
fn answer(n: usize, request: &Value) -> Value {
    let mut answer = line(n);
    answer["id"] = request["id"].clone();
    answer
}

#[test]
fn a_real_0_89_1_start_parses_and_ignores_auth_status() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        let info = client.initialize().await.unwrap();
        assert_eq!(info.version.as_deref(), Some("0.89.1"));
        assert!(info.capabilities.load_session);
        assert!(info.capabilities.session.resume);
        assert_eq!(info.auth_methods.len(), 2);
        let start = client
            .new_session(NewSession {
                cwd: cwd(),
                additional_directories: Vec::new(),
                mcp_servers: Vec::new(),
                meta: None,
            })
            .await
            .unwrap();
        let modes = start.modes.expect("modes");
        assert_eq!(modes.current, "default");
        assert!(modes.available.iter().any(|mode| mode.id == "plan"));
        let ids: Vec<&str> = start.config_options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["mode", "model", "effort", "fast"]);
        let refused = client
            .prompt(
                &start.session,
                turn(1),
                vec![ContentBlock::Text {
                    text: "Say hi.".to_owned(),
                }],
            )
            .await;
        assert!(
            matches!(refused, Err(Error::AuthRequired { .. })),
            "{refused:?}"
        );
    };
    let script = async {
        let init = agent.read().await;
        assert_eq!(init["method"], "initialize");
        agent.send(&answer(0, &init));
        agent.send(&line(1));
        let new = agent.read().await;
        assert_eq!(new["method"], "session/new");
        agent.send(&answer(2, &new));
        agent.send(&line(3));
        let prompt = agent.read().await;
        assert_eq!(prompt["method"], "session/prompt");
        agent.send(&answer(4, &prompt));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let seen = drain(&events);
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, AcpEvent::Warning { .. })),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|event| matches!(event, AcpEvent::Update { .. })),
        "the commands update: {seen:?}"
    );
}

#[test]
fn a_refused_session_new_names_its_method_and_data_keys() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let refused = client
            .new_session(NewSession {
                cwd: cwd(),
                additional_directories: Vec::new(),
                mcp_servers: Vec::new(),
                meta: None,
            })
            .await
            .unwrap_err();
        let Error::Rpc {
            method,
            code,
            message,
            data,
            ..
        } = &refused
        else {
            panic!("{refused:?}");
        };
        assert_eq!(
            (method.as_str(), *code, data.as_str()),
            ("session/new", -32602, "keys [cwd]")
        );
        assert!(message.starts_with("Invalid params: `cwd` does not exist"));
        assert_eq!(refused.code(), ErrorCode::AgentProtocolError);
    };
    let script = async {
        let init = agent.read().await;
        agent.send(&answer(0, &init));
        let new = agent.read().await;
        agent.send(&answer(5, &new));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}
