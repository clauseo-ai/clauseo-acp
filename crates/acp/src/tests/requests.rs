//! Answers to the agent's requests: each pending request has its own key, and a URL
//! elicitation's accept is exactly `{"action": "accept"}`.

use clauseo_protocol::{AgentChoiceId, ElicitationRequest, ElicitationResponse};

use super::*;

fn permission_request(id: &str, tool_call: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {
        "sessionId": "s-1",
        "toolCall": {"toolCallId": tool_call, "title": "Edit clause", "kind": "edit",
            "status": "pending"},
        "options": [
            {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"},
        ],
    }})
}

/// Two permission requests pending at once get two keys, and the answer given for each key
/// goes to its own request.
#[test]
fn pending_permission_requests_have_distinct_keys() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let mut keys: Vec<(String, PendingKey)> = Vec::new();
        while keys.len() < 2 {
            if let AcpEvent::PermissionRequested {
                key,
                acp_tool_call_id,
                ..
            } = events.recv().await.unwrap()
            {
                keys.push((acp_tool_call_id.to_string(), key));
            }
        }
        keys.sort();
        let [(first_call, first), (second_call, second)] = <[_; 2]>::try_from(keys).unwrap();
        assert_eq!(
            (first_call.as_str(), second_call.as_str()),
            ("tc-1", "tc-2")
        );
        assert_ne!(first, second);
        let reject = AgentChoiceId::new("reject-once").unwrap();
        client.answer_permission(second, reject).await.unwrap();
        let allow = AgentChoiceId::new("allow-once").unwrap();
        client.answer_permission(first, allow).await.unwrap();
    };
    let script = async {
        agent.start().await;
        agent.send(&permission_request("perm-1", "tc-1"));
        agent.send(&permission_request("perm-2", "tc-2"));
        let answer = agent.read().await;
        assert_eq!(answer["id"], "perm-2", "{answer}");
        assert_eq!(answer["result"]["outcome"]["optionId"], "reject-once");
        let answer = agent.read().await;
        assert_eq!(answer["id"], "perm-1", "{answer}");
        assert_eq!(answer["result"]["outcome"]["optionId"], "allow-once");
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// A permission request carrying `tool_call` (the `toolCall` object) and the two options.
fn permission_with(id: &str, tool_call: Value) -> Value {
    let mut request = permission_request(id, "tc-paths");
    request["params"]["toolCall"] = tool_call;
    request
}

/// `PermissionRequested.paths` holds every `locations[].path`, then `rawInput.file_path` and
/// `rawInput.notebook_path` when they are strings, without repeats, as the agent wrote them:
/// claude-agent-acp's Edit (the location repeats `file_path`) and `NotebookEdit`, codex-acp's file
/// change (locations only, which may be empty), a relative path, and a request with neither.
#[test]
fn permission_requests_carry_the_paths_they_name() {
    let call = |raw_input: Value, locations: Value| {
        json!({"toolCallId": "tc-paths", "title": "Edit", "kind": "edit", "status": "pending",
            "rawInput": raw_input, "locations": locations})
    };
    let requests = [
        call(
            json!({"file_path": "/m/Brief.docx", "old_string": "a", "new_string": "b"}),
            json!([{"path": "/m/Brief.docx"}]),
        ),
        call(
            json!({"notebook_path": "/m/n.ipynb", "file_path": 7, "new_source": "/m/x"}),
            json!([{"path": "/m/blocked.txt"}]),
        ),
        call(
            json!({"changes": {"/m/a.md": {}}}),
            json!([{"path": "/m/a.md"}, {"path": "notes/b.md", "line": 3}, {"path": "/m/a.md"}]),
        ),
        call(json!({"changes": {}}), json!([])),
        json!({"toolCallId": "tc-paths", "title": "Edit", "kind": "edit", "status": "pending"}),
    ];
    let count = requests.len();
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let mut seen: Vec<Vec<String>> = Vec::new();
        while seen.len() < count {
            if let AcpEvent::PermissionRequested { key, paths, .. } = events.recv().await.unwrap() {
                let paths = paths.iter().map(|p| p.display().to_string()).collect();
                seen.push(paths);
                let reject = AgentChoiceId::new("reject-once").unwrap();
                client.answer_permission(key, reject).await.unwrap();
            }
        }
        assert_eq!(
            seen,
            [
                vec!["/m/Brief.docx"],
                vec!["/m/blocked.txt", "/m/n.ipynb"],
                vec!["/m/a.md", "notes/b.md"],
                vec![],
                vec![],
            ]
        );
    };
    let script = async {
        agent.start().await;
        for (n, tool_call) in requests.into_iter().enumerate() {
            agent.send(&permission_with(&format!("perm-{n}"), tool_call));
            let answer = agent.read().await;
            assert_eq!(answer["result"]["outcome"]["optionId"], "reject-once");
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// Accepting a URL elicitation answers `{"action": "accept"}` with no `content`, even when the
/// host passes values: a URL elicitation collects nothing.
#[test]
fn url_elicitation_accept_carries_no_content() {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let key = loop {
            if let AcpEvent::ElicitationRequested { key, request, .. } =
                events.recv().await.unwrap()
            {
                assert!(
                    matches!(request, ElicitationRequest::Url { .. }),
                    "{request:?}"
                );
                break key;
            }
        };
        let values = serde_json::from_value(json!({"party": "Tenant"})).unwrap();
        client
            .answer_elicitation(key, ElicitationResponse::Accept { values })
            .await
            .unwrap();
    };
    let script = async {
        agent.start().await;
        agent.send(
            &json!({"jsonrpc": "2.0", "id": "el-req-1", "method": "elicitation/create",
            "params": {"mode": "url", "sessionId": "s-1", "elicitationId": "el-1",
                "message": "Sign the contract in your browser",
                "url": "https://sign.example.com/contract/42"}}),
        );
        let answer = agent.read().await;
        assert_eq!(
            answer,
            json!({"jsonrpc": "2.0", "id": "el-req-1", "result": {"action": "accept"}})
        );
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}
