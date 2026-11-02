//! What R15 added for codex-acp: `session/resume`, item plans by plan id, and
//! `_codex/hooks/list`.

use super::*;
use crate::event::{Origin, ProtocolWarning};

/// The request for a resume or load of session `s-1` in [`cwd`].
fn stored() -> LoadSession {
    LoadSession {
        session: session(),
        cwd: cwd(),
        mcp_servers: Vec::new(),
        meta: None,
    }
}

/// `session/resume` keeps the session id, sends the folder and the servers, and tags what the
/// agent sends next as idle, never as replay; a failed resume forgets the session.
#[test]
fn resume_session_sends_session_resume_and_replays_nothing() {
    let (client, events, driver, mut agent) = pipes();
    let events_ref = &events;
    let calls = async move {
        client.initialize().await.unwrap();
        let start = client.resume_session(stored()).await.unwrap();
        assert_eq!(start.session, session());
        assert_eq!(start.modes.unwrap().current.as_str(), "workspace-write");
        let event = events_ref.recv().await.unwrap();
        let AcpEvent::Update { origin, .. } = event else {
            panic!("expected an update, got {event:?}");
        };
        assert_eq!(origin, Origin::Idle);
        // A resume the agent refuses leaves no session behind.
        let refused = client
            .resume_session(LoadSession {
                session: AgentSessionId::new("s-2").unwrap(),
                ..stored()
            })
            .await;
        assert!(matches!(refused, Err(Error::Rpc { .. })), "{refused:?}");
        let unknown = AgentSessionId::new("s-2").unwrap();
        let prompt = client.prompt(&unknown, turn(1), Vec::new()).await;
        assert_eq!(prompt, Err(Error::UnknownSession));
    };
    let script = async {
        agent
            .initialize_with(json!({"sessionCapabilities": {"resume": {}}}))
            .await;
        let resume = agent.read().await;
        assert_eq!(resume["method"], "session/resume");
        assert_eq!(resume["params"]["sessionId"], "s-1");
        assert_eq!(resume["params"]["cwd"], "/matters/lease");
        agent.update(&json!({"sessionId": "s-1", "update": {
            "sessionUpdate": "available_commands_update", "availableCommands": []}}));
        agent.respond(
            &resume,
            &json!({"modes": {"currentModeId": "workspace-write", "availableModes": [
                {"id": "workspace-write", "name": "Workspace access"}]}}),
        );
        let second = agent.read().await;
        assert_eq!(second["method"], "session/resume");
        agent.send(&json!({"jsonrpc": "2.0", "id": second["id"],
            "error": {"code": -32603, "message": "no such thread"}}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    assert_eq!(drain(&events), []);
}

fn items(plan: &str, contents: &[&str]) -> Value {
    let entries: Vec<Value> = contents
        .iter()
        .map(|content| json!({"content": content, "priority": "medium", "status": "pending"}))
        .collect();
    json!({"sessionUpdate": "plan_update",
        "plan": {"type": "items", "planId": plan, "entries": entries}})
}

fn removed(plan: &str) -> Value {
    json!({"sessionUpdate": "plan_removed", "planId": plan})
}

/// What the host sees of a plan: `Plan` for the item plan updated last, an empty `Plan` when
/// that one is removed, nothing for removing another, and a warning for a markdown or file plan.
#[test]
fn item_plans_by_plan_id() {
    let (client, events, driver, mut agent) = pipes();
    let updates = [
        items("p-a", &["Read the lease"]),
        items("p-b", &["Draft clause 4", "Check dates"]),
        // Not the shown plan: nothing.
        removed("p-a"),
        json!({"sessionUpdate": "plan_update",
            "plan": {"type": "markdown", "planId": "p-c", "content": "# Plan"}}),
        json!({"sessionUpdate": "plan_update",
            "plan": {"type": "file", "planId": "p-d", "uri": "file:///plan.md"}}),
        // The shown plan, still `p-b` after the markdown and file plans: an empty plan.
        removed("p-b"),
        // Removed already: nothing.
        removed("p-b"),
        items("p-e", &["Send"]),
        // A whole plan replaces the item plan, so removing `p-e` then shows nothing new.
        json!({"sessionUpdate": "plan", "entries": [
            {"content": "Whole", "priority": "medium", "status": "pending"}]}),
        removed("p-e"),
    ];
    let calls = async move {
        handshake(&client).await;
        drop(client);
    };
    let script = async {
        agent.start().await;
        for update in &updates {
            agent.update(&json!({"sessionId": "s-1", "update": update}));
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let seen: Vec<String> = drain(&events)
        .into_iter()
        .map(|event| match event {
            AcpEvent::Update {
                update: AgentUpdate::Plan(entries),
                ..
            } => format!(
                "plan {:?}",
                entries
                    .iter()
                    .map(|e| e.content.as_str())
                    .collect::<Vec<_>>()
            ),
            AcpEvent::Warning {
                warning: ProtocolWarning::UnknownUpdate { name },
                ..
            } => format!("unknown {name}"),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        seen,
        [
            r#"plan ["Read the lease"]"#,
            r#"plan ["Draft clause 4", "Check dates"]"#,
            "unknown plan_update",
            "unknown plan_update",
            "plan []",
            r#"plan ["Send"]"#,
            r#"plan ["Whole"]"#,
        ]
    );
}

/// `_codex/hooks/list` goes out with the folder as `cwd`, and the answer comes back as it is.
#[test]
fn codex_hooks_list_sends_the_folder_and_returns_the_answer() {
    let (client, _events, driver, mut agent) = pipes();
    let answer = json!({"cwd": "/matters/lease", "hooks": [
        {"key": "k", "source": "sessionFlags", "currentHash": "sha256:00", "trustStatus": "trusted"}]});
    let expected = answer.clone();
    let calls = async move {
        handshake(&client).await;
        let listed = client.codex_hooks_list(&cwd()).await.unwrap();
        assert_eq!(listed, expected);
    };
    let script = async {
        agent.start().await;
        let list = agent.read().await;
        assert_eq!(list["method"], "_codex/hooks/list");
        assert_eq!(list["params"], json!({"cwd": "/matters/lease"}));
        agent.respond(&list, &answer);
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}
