//! MCP servers and `_meta` on the wire, for `session/new` and `session/load`.

use super::*;

fn server() -> McpServer {
    McpServer::Stdio {
        name: "research".to_owned(),
        command: PathBuf::from("/opt/research/server"),
        args: vec!["--stdio".to_owned()],
        env: vec![("REGION".to_owned(), "uk".to_owned())],
    }
}

fn meta() -> serde_json::Map<String, Value> {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "claudeCode".to_owned(),
        json!({"options": {"strictMcpConfig": true}}),
    );
    meta
}

/// The server as claude-agent-acp reads a stdio one: no `type` field (one sent with
/// `"type": "stdio"` is dropped by the adapter).
fn expected_server() -> Value {
    json!({
        "name": "research",
        "command": "/opt/research/server",
        "args": ["--stdio"],
        "env": [{"name": "REGION", "value": "uk"}],
    })
}

#[test]
fn a_stdio_server_and_meta_go_on_new_and_load_with_no_type_field() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let new = client.new_session(NewSession {
            cwd: cwd(),
            mcp_servers: vec![server()],
            meta: Some(meta()),
        });
        assert_eq!(new.await.unwrap().session.as_str(), "s-1");
        let load = client.load_session(LoadSession {
            session: AgentSessionId::new("s-0").unwrap(),
            cwd: cwd(),
            mcp_servers: vec![server()],
            meta: Some(meta()),
        });
        assert_eq!(load.await.unwrap().session.as_str(), "s-0");
    };
    let script = async {
        agent.initialize().await;
        let new = agent.read().await;
        assert_eq!(new["method"], "session/new");
        assert_eq!(new["params"]["mcpServers"], json!([expected_server()]));
        assert_eq!(new["params"]["mcpServers"][0].get("type"), None);
        assert_eq!(new["params"]["_meta"], Value::Object(meta()));
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        let load = agent.read().await;
        assert_eq!(load["method"], "session/load");
        assert_eq!(load["params"]["sessionId"], "s-0");
        assert_eq!(load["params"]["mcpServers"], json!([expected_server()]));
        assert_eq!(load["params"]["mcpServers"][0].get("type"), None);
        assert_eq!(load["params"]["_meta"], Value::Object(meta()));
        agent.respond(&load, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn load_without_meta_sends_no_meta() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let load = client.load_session(LoadSession {
            session: AgentSessionId::new("s-0").unwrap(),
            cwd: cwd(),
            mcp_servers: Vec::new(),
            meta: None,
        });
        load.await.unwrap();
    };
    let script = async {
        agent.initialize().await;
        let load = agent.read().await;
        assert_eq!(load["params"]["mcpServers"], json!([]));
        assert_eq!(load["params"].get("_meta"), None);
        agent.respond(&load, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}
