//! An error the agent answers a start request with is a failed launch (`agent.launch_failed`),
//! never `agent.protocol_error`: the agent's error read fine. Only an answer that does not
//! decode is unreadable (`Error::Protocol`).

use super::*;

/// `session/new` in [`cwd`].
fn new_session() -> NewSession {
    NewSession {
        cwd: cwd(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        meta: None,
    }
}

/// Runs `initialize` (answered by `init`, or as [`Agent::initialize`] does when `None`) then,
/// when `init` succeeded, `session/new` answered by `new`; gives the first error.
fn start_with(init: Option<Value>, new: Value) -> Error {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await?;
        client.new_session(new_session()).await.map(|_| ())
    };
    let script = async {
        match init {
            None => agent.initialize().await,
            Some(answer) => {
                let request = agent.read().await;
                let mut answer = answer;
                answer["jsonrpc"] = json!("2.0");
                answer["id"] = request["id"].clone();
                agent.send(&answer);
                return;
            }
        }
        let request = agent.read().await;
        assert_eq!(request["method"], "session/new");
        let mut answer = new;
        answer["jsonrpc"] = json!("2.0");
        answer["id"] = request["id"].clone();
        agent.send(&answer);
    };
    let (_driver, result, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    result.unwrap_err()
}

/// Rohan's case: Claude answered `session/new` with -32603 (internal error).
#[test]
fn an_internal_error_to_session_new_is_a_failed_launch() {
    let error = start_with(
        None,
        json!({"error": {"code": -32603, "message": "Internal error", "data": {"details": "x"}}}),
    );
    assert!(
        matches!(&error, Error::Rpc { method, code: -32603, .. } if method == "session/new"),
        "{error:?}"
    );
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
}

#[test]
fn invalid_params_and_method_not_found_to_session_new_are_failed_launches() {
    for code in [-32602, -32601] {
        let error = start_with(None, json!({"error": {"code": code, "message": "refused"}}));
        assert!(matches!(error, Error::Rpc { .. }), "{error:?}");
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{code}");
    }
}

#[test]
fn an_error_to_initialize_is_a_failed_launch() {
    let error = start_with(
        Some(json!({"error": {"code": -32603, "message": "Internal error"}})),
        Value::Null,
    );
    assert!(
        matches!(&error, Error::Rpc { method, .. } if method == "initialize"),
        "{error:?}"
    );
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
}

/// ACP's own auth code (-32000) stays `agent.auth_required`, so the turn is held for sign-in.
#[test]
fn auth_required_to_session_new_stays_a_sign_in() {
    let error = start_with(
        None,
        json!({"error": {"code": -32000, "message": "Authentication required"}}),
    );
    assert!(matches!(error, Error::AuthRequired { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::AgentAuthRequired);
}

/// An answer that does not decode (no `sessionId`) is the one unreadable case.
#[test]
fn an_answer_that_does_not_decode_is_a_protocol_error() {
    let error = start_with(None, json!({"result": {"notASession": 1}}));
    assert!(
        matches!(&error, Error::Protocol(why) if why.contains("session/new")),
        "{error:?}"
    );
    assert_eq!(error.code(), ErrorCode::AgentProtocolError);
}

#[test]
fn each_start_method_is_a_failed_launch_and_others_are_not() {
    let rpc = |method: &str| Error::Rpc {
        method: method.to_owned(),
        code: -32603,
        message: String::new(),
        data: String::new(),
        data_text: None,
        data_fields: crate::RpcData::default(),
    };
    for method in crate::error::START_METHODS {
        assert_eq!(rpc(method).code(), ErrorCode::AgentLaunchFailed, "{method}");
    }
    // A question's error is the answer's failure, never unreadable; any other request's stays.
    assert_eq!(rpc("session/prompt").code(), ErrorCode::AgentAnswerFailed);
    assert_eq!(
        rpc("session/set_mode").code(),
        ErrorCode::AgentProtocolError
    );
}
