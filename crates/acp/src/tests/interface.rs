//! Interface items over in-memory pipes: warnings name their session, the line filter, the
//! frame tap, refused requests, `set_mode`, and the bound on lines in flight.

use clauseo_protocol::AgentChoiceId;
use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;

use super::*;
use crate::line::MAX_LINES_IN_FLIGHT;

fn session() -> AgentSessionId {
    AgentSessionId::new("s-1").unwrap()
}

fn new_session() -> NewSession {
    NewSession {
        cwd: cwd(),
        mcp_servers: Vec::new(),
        meta: None,
    }
}

fn warning(session: Option<AgentSessionId>, warning: ProtocolWarning) -> AcpEvent {
    AcpEvent::Warning { session, warning }
}

#[test]
fn session_failure_warning_names_its_session() {
    let (client, events, driver, mut agent) = pipes();
    let turn = TurnId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap();
    let failure = json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "f", "revision": 1, "category": "context", "severity": "error",
        "title": "The context is full.", "actions": ["new_session"],
    }}}});
    let calls = async move {
        client.initialize().await.unwrap();
        client.new_session(new_session()).await.unwrap();
        let end = client.prompt(&session(), turn, Vec::new()).await.unwrap();
        assert_eq!(end.limit, None);
    };
    let script = async {
        agent.start().await;
        let prompt = agent.read().await;
        assert_eq!(prompt["method"], "session/prompt");
        agent.update(&json!({"sessionId": "s-1", "update": {
            "sessionUpdate": "session_info_update", "_meta": failure,
        }}));
        agent.respond(
            &prompt,
            &json!({"stopReason": "end_turn", "_meta": failure}),
        );
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let expected = warning(
        Some(session()),
        ProtocolWarning::SessionFailure {
            category: Some("context".to_owned()),
            severity: Some("error".to_owned()),
            title: Some("The context is full.".to_owned()),
        },
    );
    // One from the `session_info_update`, one from the prompt response.
    assert_eq!(drain(&events), [expected.clone(), expected]);
}

#[test]
fn update_warnings_name_their_session_and_set_mode_round_trips() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let mode = AgentChoiceId::new("plan").unwrap();
        let unknown = AgentSessionId::new("s-9").unwrap();
        let refused = client.set_mode(&unknown, mode.clone()).await;
        assert_eq!(refused, Err(Error::UnknownSession));
        client.new_session(new_session()).await.unwrap();
        client.set_mode(&session(), mode).await.unwrap();
    };
    let script = async {
        agent.start().await;
        // `toolCallId` must be a string: the update does not decode. (A bad `kind` decodes as
        // `other`.)
        agent.update(&json!({"sessionId": "s-1", "update": {
            "sessionUpdate": "tool_call", "toolCallId": 5, "title": "Read",
        }}));
        agent.update(&json!({"sessionId": "s-1", "update": {"content": {}}}));
        agent.update(&json!({"sessionId": "s-1"}));
        agent.update(&json!({"sessionId": "s-1", "update": {"sessionUpdate": "hologram"}}));
        agent.update(&json!({"sessionId": "", "update": {"sessionUpdate": "plan", "entries": []}}));
        agent.send(&json!({"jsonrpc": "2.0", "method": "elicitation/complete", "params": {}}));
        let set = agent.read().await;
        assert_eq!(set["method"], "session/set_mode");
        assert_eq!(set["params"], json!({"sessionId": "s-1", "modeId": "plan"}));
        agent.respond(&set, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let summary: Vec<_> = drain(&events)
        .into_iter()
        .map(|event| match event {
            AcpEvent::Warning {
                session,
                warning: ProtocolWarning::InvalidUpdate { name, .. },
            } => (session, name),
            AcpEvent::Warning {
                session,
                warning: ProtocolWarning::UnknownUpdate { name },
            } => (session, Some(format!("unknown {name}"))),
            other @ (AcpEvent::Update { .. }
            | AcpEvent::PermissionRequested { .. }
            | AcpEvent::ElicitationRequested { .. }
            | AcpEvent::ElicitationCompleted { .. }
            | AcpEvent::Limit { .. }
            | AcpEvent::Warning { .. }) => panic!("expected a warning, got {other:?}"),
        })
        .collect();
    let named = |name: &str| Some(name.to_owned());
    assert_eq!(
        summary,
        [
            (Some(session()), named("tool_call")),
            (Some(session()), None),
            (Some(session()), None),
            (Some(session()), named("unknown hologram")),
            (None, named("plan")),
            (None, named("elicitation/complete")),
        ]
    );
}

#[test]
fn json_that_is_not_json_rpc_is_a_malformed_line() {
    let (client, events, driver, mut agent) = pipes();
    let lines = [
        json!({"level": "info", "msg": "ready"}),
        json!(42),
        json!([]),
        // ACP sends no batches.
        json!([{"jsonrpc": "2.0", "method": "session/update", "params": {}}]),
    ];
    let calls = async move {
        client.initialize().await.unwrap();
        client.new_session(new_session()).await.unwrap();
    };
    let script = async {
        agent.initialize().await;
        let new = agent.read().await;
        for line in &lines {
            agent.send(line);
        }
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        // Nothing went back for those lines.
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let expected: Vec<_> = lines
        .iter()
        .map(|line| {
            let bytes = serde_json::to_string(line).unwrap().len();
            warning(None, ProtocolWarning::MalformedLine { bytes })
        })
        .collect();
    assert_eq!(drain(&events), expected);
}

#[test]
fn frame_tap_keeps_what_fits_and_counts_the_rest() {
    let (tap, frames) = async_channel::bounded(2);
    let options = ConnectOptions {
        frame_tap: Some(tap),
    };
    let (client, _events, driver, mut agent) = pipes_with(options);
    let calls = async move {
        client.initialize().await.unwrap();
        let refused = client.authenticate("browser").await;
        assert!(
            matches!(refused, Err(Error::Rpc { code: -32603, .. })),
            "{refused:?}"
        );
        // Dropped: the initialize response, the authenticate request and its error.
        assert_eq!(client.frames_dropped(), 3);
    };
    let script = async {
        let init = agent.read().await;
        agent
            .stdout
            .unbounded_send(Ok(b"not json\n".to_vec()))
            .unwrap();
        agent.respond(&init, &json!({"protocolVersion": 1}));
        let auth = agent.read().await;
        assert_eq!(auth["method"], "authenticate");
        agent.send(&json!({"jsonrpc": "2.0", "id": auth["id"],
            "error": {"code": -32603, "message": "no browser"}}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let kept: Vec<_> = std::iter::from_fn(|| frames.try_recv().ok()).collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].direction, FrameDirection::Out);
    assert!(
        kept[0].line.contains(r#""method":"initialize""#),
        "{kept:?}"
    );
    let not_json = RawFrame {
        direction: FrameDirection::In,
        line: "not json".to_owned(),
    };
    assert_eq!(kept[1], not_json);
}

#[test]
fn terminal_auth_method_sends_nothing() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        let info = client.initialize().await.unwrap();
        assert_eq!(info.name, None);
        assert_eq!(
            info.auth_methods,
            [AuthMethod {
                id: "login".into(),
                name: "Log in".to_owned(),
                description: None,
                kind: AuthKind::Terminal {
                    args: vec!["--login".to_owned()],
                    env: Vec::new(),
                },
            }]
        );
        let refused = client.authenticate("login").await;
        assert_eq!(refused, Err(Error::TerminalAuthMethod));
    };
    let script = async {
        let init = agent.read().await;
        let method = json!({"type": "terminal", "id": "login", "name": "Log in",
            "args": ["--login"], "env": {}});
        agent.respond(
            &init,
            &json!({"protocolVersion": 1, "authMethods": [method]}),
        );
        // No `authenticate` before stdin closes.
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn request_scoped_and_unknown_elicitations_are_refused() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        client.new_session(new_session()).await.unwrap();
        let mode = AgentChoiceId::new("plan").unwrap();
        client.set_mode(&session(), mode).await.unwrap();
    };
    let script = async {
        agent.start().await;
        let schema = json!({"type": "object", "properties": {}});
        let requests = [
            json!({"mode": "form", "requestId": "r-1", "message": "Which?",
                "requestedSchema": schema}),
            json!({"mode": "telepathy", "sessionId": "s-1", "message": "Which?"}),
        ];
        for (id, params) in ["el-1", "el-2"].into_iter().zip(requests) {
            agent.send(
                &json!({"jsonrpc": "2.0", "id": id, "method": "elicitation/create",
                "params": params}),
            );
        }
        // The two refusals and `session/set_mode`, whose order on stdin is not fixed.
        let mut refused = Vec::new();
        for _ in 0..3 {
            let message = agent.read().await;
            if message["method"] == "session/set_mode" {
                agent.respond(&message, &json!({}));
            } else {
                assert_eq!(message["error"]["code"], -32602, "{message}");
                refused.push(message["id"].clone());
            }
        }
        refused.sort_by_key(ToString::to_string);
        assert_eq!(refused, [json!("el-1"), json!("el-2")]);
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    assert_eq!(drain(&events), []);
}

#[test]
fn lines_in_flight_are_bounded_while_the_host_drains_nothing() {
    const SENT: usize = 5_000;
    let (tap, frames) = async_channel::unbounded();
    let options = ConnectOptions {
        frame_tap: Some(tap),
    };
    let (client, events, driver, mut agent) = pipes_with(options);
    let mut pool = LocalPool::new();
    pool.spawner()
        .spawn_local(async move { drop(driver.await) })
        .unwrap();
    pool.run_until(async {
        let calls = async {
            client.initialize().await.unwrap();
            client.new_session(new_session()).await.unwrap();
        };
        futures::join!(calls, agent.start());
    });
    for n in 0..SENT {
        agent.update(&json!({"sessionId": "s-1", "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": n.to_string()},
        }}));
    }
    pool.run_until_stalled();
    let read_in = std::iter::from_fn(|| frames.try_recv().ok())
        .filter(|frame| frame.direction == FrameDirection::In)
        .count();
    // Less the two responses.
    let pulled = read_in - 2;
    assert!(pulled > EVENT_CAPACITY, "{pulled}");
    assert!(
        pulled <= EVENT_CAPACITY + MAX_LINES_IN_FLIGHT + 1,
        "{pulled}"
    );
    // Draining lets every line through, in order.
    let mut received = 0;
    while received < SENT {
        let drained = drain(&events);
        assert!(!drained.is_empty(), "stalled after {received} events");
        for event in drained {
            let AcpEvent::Update {
                update:
                    AgentUpdate::MessageChunk {
                        content: ContentBlock::Text { text },
                        ..
                    },
                ..
            } = event
            else {
                panic!("expected a chunk, got {event:?}");
            };
            assert_eq!(text, received.to_string());
            received += 1;
        }
        pool.run_until_stalled();
    }
    drop(client);
}
