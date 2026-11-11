//! Http MCP servers over real pipes: sent with their headers to an agent that advertises
//! `mcpCapabilities.http`, left out (by name in the log) for one that does not. No header value,
//! nor the URL, reaches a `clauseo_*` log line.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_acp::McpServer;
use tracing_subscriber::fmt::MakeWriter;

use crate::harness::{self, Ctx};

/// Every formatted log line, kept for the test to read.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    /// The lines from Clauseo's own crates (the only ones the host's log file keeps) and from
    /// this test. The SDK's own TRACE lines carry each JSON-RPC line whole and are never kept.
    fn clauseo_lines(&self) -> Vec<String> {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        String::from_utf8(bytes)
            .expect("utf-8 log")
            .lines()
            .filter(|line| line.contains(" clauseo_") || line.contains(" stdio::"))
            .map(str::to_owned)
            .collect()
    }
}

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

const SECRETS: [&str; 3] = ["s3cret-header", "firm-7", "mcp.example.com"];

fn servers() -> Vec<McpServer> {
    vec![
        McpServer::Stdio {
            name: "research".to_owned(),
            command: PathBuf::from("/opt/research/server"),
            args: vec!["--stdio".to_owned()],
            env: Vec::new(),
        },
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
        },
    ]
}

/// Plays `scenario` with `initialize` then `session/new` carrying [`servers`], every
/// `clauseo_*` event at any level captured; returns the captured lines.
fn new_session_logged(scenario: &str) -> Vec<String> {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let outcome = harness::run(&harness::local(scenario), async |ctx: Ctx| {
            ctx.client.initialize().await.unwrap();
            let mut request = ctx.new_session();
            request.mcp_servers = servers();
            tracing::debug!(?request, "session/new");
            let start = ctx.client.new_session(request).await.unwrap();
            assert_eq!(start.session.as_str(), "sess-1");
        });
        outcome.assert_clean();
    });
    captured.clauseo_lines()
}

#[test]
fn an_http_server_goes_with_its_headers_to_an_agent_with_http() {
    let lines = new_session_logged("mcp-http");
    assert!(
        lines.iter().any(|line| line.contains("docs")),
        "the request's Debug line names the server: {lines:?}"
    );
    for secret in SECRETS {
        assert!(
            lines.iter().all(|line| !line.contains(secret)),
            "{secret} logged: {lines:?}"
        );
    }
}

#[test]
fn an_http_server_is_left_out_by_name_for_an_agent_without_http() {
    let lines = new_session_logged("mcp-http-refused");
    let left_out: Vec<_> = lines
        .iter()
        .filter(|line| line.contains("WARN") && line.contains("left out an http MCP server"))
        .collect();
    assert_eq!(left_out.len(), 1, "{lines:?}");
    assert!(left_out[0].contains("server=\"docs\""), "{left_out:?}");
    for secret in SECRETS {
        assert!(
            lines.iter().all(|line| !line.contains(secret)),
            "{secret} logged: {lines:?}"
        );
    }
}
