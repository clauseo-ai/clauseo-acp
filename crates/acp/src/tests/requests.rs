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
