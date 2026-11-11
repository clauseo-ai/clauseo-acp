//! MCP servers, `additionalDirectories` and `_meta` on the wire, for `session/new` and
//! `session/load`.

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

fn directories() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/data/run/s1"),
        PathBuf::from("/data/agent-state/m1/notes"),
    ]
}

#[test]
fn a_stdio_server_directories_and_meta_go_on_new_and_load_with_no_type_field() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let new = client.new_session(NewSession {
            cwd: cwd(),
            additional_directories: directories(),
            mcp_servers: vec![server()],
            meta: Some(meta()),
        });
        assert_eq!(new.await.unwrap().session.as_str(), "s-1");
        let load = client.load_session(LoadSession {
            session: AgentSessionId::new("s-0").unwrap(),
            cwd: cwd(),
            additional_directories: directories(),
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
        let sent = json!(["/data/run/s1", "/data/agent-state/m1/notes"]);
        assert_eq!(new["params"]["additionalDirectories"], sent);
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        let load = agent.read().await;
        assert_eq!(load["method"], "session/load");
        assert_eq!(load["params"]["sessionId"], "s-0");
        assert_eq!(load["params"]["mcpServers"], json!([expected_server()]));
        assert_eq!(load["params"]["mcpServers"][0].get("type"), None);
        assert_eq!(load["params"]["_meta"], Value::Object(meta()));
        assert_eq!(load["params"]["additionalDirectories"], sent);
        agent.respond(&load, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn load_without_meta_or_directories_sends_neither() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let load = client.load_session(LoadSession {
            session: AgentSessionId::new("s-0").unwrap(),
            cwd: cwd(),
            additional_directories: Vec::new(),
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
        assert_eq!(load["params"].get("additionalDirectories"), None);
        agent.respond(&load, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

fn http_server() -> McpServer {
    McpServer::Http {
        name: "docs".to_owned(),
        url: "https://mcp.example.com/mcp".to_owned(),
        headers: vec![
            (
                "Authorization".to_owned(),
                "Bearer s3cret-header".to_owned(),
            ),
            ("X-Tenant".to_owned(), "firm-7".to_owned()),
        ],
    }
}

/// ACP v1's `McpServerHttp` (agent-client-protocol-schema 1.11.0, `v1/agent.rs`): tagged
/// `"type": "http"`, `headers` as name and value pairs and always present. codex-acp 2.2.2's
/// zod schema requires `headers`; claude-agent-acp 0.89.1 reads `type`, `url` and `headers`.
fn expected_http_server() -> Value {
    json!({
        "type": "http",
        "name": "docs",
        "url": "https://mcp.example.com/mcp",
        "headers": [
            {"name": "Authorization", "value": "Bearer s3cret-header"},
            {"name": "X-Tenant", "value": "firm-7"},
        ],
    })
}

fn load(session: &str, servers: Vec<McpServer>) -> LoadSession {
    LoadSession {
        session: AgentSessionId::new(session).unwrap(),
        cwd: cwd(),
        additional_directories: Vec::new(),
        mcp_servers: servers,
        meta: None,
    }
}

#[test]
fn an_http_server_goes_tagged_with_its_headers_on_new_load_and_resume_when_the_agent_has_http() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let new = client.new_session(NewSession {
            cwd: cwd(),
            additional_directories: Vec::new(),
            mcp_servers: vec![server(), http_server()],
            meta: None,
        });
        new.await.unwrap();
        let both = || vec![server(), http_server()];
        client.load_session(load("s-0", both())).await.unwrap();
        client.resume_session(load("s-2", both())).await.unwrap();
    };
    let script = async {
        agent
            .initialize_with(json!({"mcpCapabilities": {"http": true, "sse": false}}))
            .await;
        let sent = json!([expected_server(), expected_http_server()]);
        let new = agent.read().await;
        assert_eq!(new["params"]["mcpServers"], sent);
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        let load = agent.read().await;
        assert_eq!(load["method"], "session/load");
        assert_eq!(load["params"]["mcpServers"], sent);
        agent.respond(&load, &json!({}));
        let resume = agent.read().await;
        assert_eq!(resume["method"], "session/resume");
        assert_eq!(resume["params"]["mcpServers"], sent);
        agent.respond(&resume, &json!({}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

#[test]
fn an_http_server_with_no_headers_still_sends_an_empty_headers_list() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        client.initialize().await.unwrap();
        let bare = McpServer::Http {
            name: "open".to_owned(),
            url: "https://open.example.com/mcp".to_owned(),
            headers: Vec::new(),
        };
        let new = client.new_session(NewSession {
            cwd: cwd(),
            additional_directories: Vec::new(),
            mcp_servers: vec![bare],
            meta: None,
        });
        new.await.unwrap();
    };
    let script = async {
        agent
            .initialize_with(json!({"mcpCapabilities": {"http": true}}))
            .await;
        let new = agent.read().await;
        let expected = json!([{
            "type": "http",
            "name": "open",
            "url": "https://open.example.com/mcp",
            "headers": [],
        }]);
        assert_eq!(new["params"]["mcpServers"], expected);
        agent.respond(&new, &json!({"sessionId": "s-1"}));
        assert!(agent.written.next().await.is_none());
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// Gate off: an agent whose `initialize` has no `mcpCapabilities.http` (absent, or false as
/// codex-acp sends `sse`) gets the stdio servers only, on new, load and resume.
#[test]
fn an_http_server_is_left_out_when_the_agent_has_no_http() {
    for capabilities in [
        json!({}),
        json!({"mcpCapabilities": {"http": false, "sse": true}}),
    ] {
        let (client, _events, driver, mut agent) = pipes();
        let calls = async move {
            client.initialize().await.unwrap();
            let new = client.new_session(NewSession {
                cwd: cwd(),
                additional_directories: Vec::new(),
                mcp_servers: vec![http_server(), server()],
                meta: None,
            });
            new.await.unwrap();
            let both = || vec![http_server(), server()];
            client.load_session(load("s-0", both())).await.unwrap();
            client.resume_session(load("s-2", both())).await.unwrap();
        };
        let script = async {
            agent.initialize_with(capabilities).await;
            let sent = json!([expected_server()]);
            let new = agent.read().await;
            assert_eq!(new["params"]["mcpServers"], sent);
            agent.respond(&new, &json!({"sessionId": "s-1"}));
            let load = agent.read().await;
            assert_eq!(load["params"]["mcpServers"], sent);
            agent.respond(&load, &json!({}));
            let resume = agent.read().await;
            assert_eq!(resume["params"]["mcpServers"], sent);
            agent.respond(&resume, &json!({}));
            assert!(agent.written.next().await.is_none());
        };
        let (driver, (), ()) =
            futures::executor::block_on(async { futures::join!(driver, calls, script) });
        assert_eq!(driver, Ok(()));
    }
}

/// `Debug` (and so any `?server` or `?request` in a log line) shows names, never env or header
/// values, nor the URL, which can carry a token.
#[test]
fn debug_shows_names_and_no_secret_values() {
    let printed = format!("{:?} {:?}", server(), http_server());
    assert!(printed.contains("research") && printed.contains("REGION"));
    assert!(printed.contains("docs") && printed.contains("Authorization"));
    for secret in ["uk\"", "s3cret-header", "firm-7", "mcp.example.com"] {
        assert!(!printed.contains(secret), "{secret} in {printed}");
    }
}
