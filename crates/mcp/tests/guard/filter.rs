//! The filter end to end: the echo stub logs every line it receives, so each test shows both what
//! the agent got back and what reached the child.

use serde_json::{Value, json};

use crate::support::{Guard, Scratch, example, guard_command};

/// A guard in front of the echo stub.
struct Session {
    scratch: Scratch,
    guard: Guard,
    next_id: u64,
    /// Every line the child must have received, in order.
    forwarded: Vec<Value>,
}

impl Session {
    fn start(test: &str) -> Self {
        let scratch = Scratch::new(test);
        let log = scratch.log();
        let guard = Guard::start(guard_command(
            &scratch,
            &example("echo_stub"),
            &[log.as_os_str()],
        ));
        Self {
            scratch,
            guard,
            next_id: 0,
            forwarded: Vec::new(),
        }
    }

    fn tool_call(&mut self, arguments: &Value) -> Value {
        self.next_id += 1;
        json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "tools/call",
            "params": {"name": "agent_browser_open", "arguments": arguments},
        })
    }

    /// Sends a call that must be refused, and returns the refusal's text.
    fn refused(&mut self, arguments: &Value) -> String {
        let call = self.tool_call(arguments);
        self.guard.send(&call);
        let answer = self.guard.read();
        assert_eq!(answer["id"], call["id"], "{answer}");
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Sends a message that must reach the child unchanged and be answered by it.
    fn passes(&mut self, message: &Value) {
        self.guard.send(message);
        let answer = self.guard.read();
        assert_eq!(answer["result"]["received"], *message, "{answer}");
        self.forwarded.push(message.clone());
    }

    /// Sends a raw line the guard must answer itself, and returns the answer.
    fn answered_raw(&mut self, line: &[u8]) -> Value {
        self.guard.send_raw(line);
        self.guard.read()
    }

    /// Closes the agent's side and checks the child got exactly the forwarded lines, then the
    /// guard's close request, whose reply never reached the agent.
    fn finish(self) {
        let (status, rest) = self.guard.finish();
        assert_eq!(status.code(), Some(0));
        assert_eq!(rest, "", "the close reply is swallowed");
        let mut expected = self.forwarded;
        expected.push(json!({
            "jsonrpc": "2.0",
            "id": "clauseo-guard-close",
            "method": "tools/call",
            "params": {"name": "agent_browser_close", "arguments": {}},
        }));
        assert_eq!(self.scratch.received(), expected);
    }
}

fn refusal(name: &str) -> String {
    format!("Clauseo does not allow {name} in the browser.")
}

#[test]
fn every_refused_key_is_answered_by_the_guard() {
    let mut session = Session::start("filter_keys");
    for key in [
        "session",
        "namespace",
        "idleTimeout",
        "restore",
        "restoreSave",
        "restoreCheckUrl",
        "restoreCheckText",
        "restoreCheckFn",
        "caCert",
        "useSystemCa",
        "clearCaCert",
        "headed",
        "screenshotDir",
        "path",
    ] {
        let text = session.refused(&json!({"url": "https://example.com", key: "x"}));
        assert_eq!(text, refusal(key));
    }
    session.finish();
}

#[test]
fn url_schemes() {
    let mut session = Session::start("filter_urls");
    for (url, name) in [
        ("file:///etc/passwd", "file URLs"),
        ("javascript:alert(1)", "javascript URLs"),
        ("data:text/html,<b>x</b>", "data URLs"),
        ("chrome://settings", "chrome URLs"),
        ("view-source:https://example.com", "view-source URLs"),
        (
            "devtools://devtools/bundled/inspector.html",
            "devtools URLs",
        ),
    ] {
        assert_eq!(session.refused(&json!({ "url": url })), refusal(name));
    }
    for url in [
        "https://example.com",
        "HTTP://127.0.0.1:8/",
        "about:blank",
        "sci.gov.in",
    ] {
        let call = session.tool_call(&json!({ "url": url }));
        session.passes(&call);
    }
    session.finish();
}

#[test]
fn extra_args() {
    let mut session = Session::start("filter_extra");
    for (tokens, name) in [
        (json!(["--headed"]), "--headed"),
        (json!(["--engine=x"]), "--engine"),
        (json!(["--proxy", "http://evil:1"]), "--proxy"),
        (json!(["https://evil.example"]), "https://evil.example"),
        (json!(["--json", 7]), "extraArgs"),
    ] {
        let text = session.refused(&json!({"url": "https://a.example", "extraArgs": tokens}));
        assert_eq!(text, refusal(name));
    }
    let call = session.tool_call(&json!({"url": "https://a.example", "extraArgs": ["--json"]}));
    session.passes(&call);
    session.finish();
}

#[test]
fn typed_values_that_read_as_flags() {
    let mut session = Session::start("filter_typed");
    let text = session.refused(&json!({"selector": "--headed"}));
    assert_eq!(text, refusal("--headed"));
    session.finish();
}

#[test]
fn malformed_lines_batches_and_duplicates() {
    let mut session = Session::start("filter_malformed");
    let parse_error = json!({"jsonrpc": "2.0", "id": null,
        "error": {"code": -32700, "message": "Parse error"}});
    assert_eq!(session.answered_raw(b"{not json"), parse_error);
    let batch = br#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#;
    assert_eq!(
        session.answered_raw(batch),
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600,
            "message": "Clauseo does not allow batched requests in the browser."}})
    );
    let duplicate = br#"{"jsonrpc":"2.0","id":"dup","method":"tools/call","params":{"name":"agent_browser_open","arguments":{"url":"https://a","url":"file:///etc/passwd"}}}"#;
    let answer = session.answered_raw(duplicate);
    assert_eq!(answer["id"], "dup");
    assert_eq!(answer["result"]["content"][0]["text"], refusal("file URLs"));
    session.finish();
}

#[test]
fn a_refused_notification_is_dropped_and_an_allowed_one_passes() {
    let mut session = Session::start("filter_notify");
    let refused = json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"headed": true}}});
    session.guard.send(&refused);
    let allowed = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    session.guard.send(&allowed);
    session.forwarded.push(allowed);
    // A request after both: its answer is the next line, so neither notification got one.
    session.passes(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    session.finish();
}

#[test]
fn an_oversize_line_is_refused_and_the_stream_goes_on() {
    let mut session = Session::start("filter_oversize");
    let mut line = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"pad":""#.to_vec();
    line.resize(16 * 1024 * 1024 + 1, b'x');
    let answer = session.answered_raw(&line);
    assert_eq!(answer["error"]["code"], -32700);
    session.passes(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    session.finish();
}

#[test]
fn allowed_calls_tools_list_and_initialize_reach_the_child_unchanged() {
    let mut session = Session::start("filter_pass");
    session.passes(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2"}}}));
    session.passes(&json!({"jsonrpc": "2.0", "id": "list", "method": "tools/list"}));
    let open = session.tool_call(&json!({"url": "https://sci.gov.in",
        "allowedDomains": ["sci.gov.in"], "timeoutMs": 30000}));
    session.passes(&open);
    session.passes(&json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": {"name": "agent_browser_fill",
            "arguments": {"selector": "#q", "text": "Section 138 -- \"cheque\" é"}}}));
    session.finish();
}

#[test]
fn when_the_child_exits_first_the_guard_exits_with_its_code() {
    let mut session = Session::start("filter_child_exits");
    session
        .guard
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "stub/exit"}));
    // stdin stays open: the guard ends because the child did, and sends no close request.
    assert_eq!(session.guard.read_rest(), "");
    assert_eq!(session.guard.wait().code(), Some(7));
    assert_eq!(session.scratch.received().len(), 1);
}
