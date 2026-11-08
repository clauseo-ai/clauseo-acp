//! The filter end to end: the echo stub logs every line it receives, so each test shows both what
//! the agent got back and what reached the child. Until a tool call starts the browser, each
//! request reaches a one-shot echo stub (the guard's lazy start), which logs it the same way.

use serde_json::{Value, json};

use crate::support::{Guard, Scratch, example, guard_command};

/// A guard in front of the echo stub.
struct Session {
    scratch: Scratch,
    guard: Guard,
    next_id: u64,
    /// Every line the child must have received, in order.
    forwarded: Vec<Value>,
    /// Whether a tool call started the browser, so the guard sends its close request at the end.
    started: bool,
}

impl Session {
    fn start(test: &str) -> Self {
        Self::start_with(test, &[])
    }

    /// With the echo stub's options after its log file (`ignore-close`, `hang`).
    fn start_with(test: &str, options: &[&str]) -> Self {
        let scratch = Scratch::new(test);
        let log = scratch.log();
        let mut args = vec![log.as_os_str()];
        args.extend(options.iter().map(std::ffi::OsStr::new));
        let guard = Guard::start(guard_command(&scratch, &example("echo_stub"), &args));
        Self {
            scratch,
            guard,
            next_id: 0,
            forwarded: Vec::new(),
            started: false,
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
        self.started |= message["method"] == "tools/call";
    }

    /// Starts the browser (Chrome's stand-in and the echo stub that keeps running) with a call.
    fn start_browser(&mut self) {
        let open = self.tool_call(&json!({"url": "https://example.com"}));
        self.passes(&open);
    }

    /// Sends a raw line the guard must answer itself, and returns the answer.
    fn answered_raw(&mut self, line: &[u8]) -> Value {
        self.guard.send_raw(line);
        self.guard.read()
    }

    /// Closes the agent's side and checks the child got exactly the forwarded lines, then, if the
    /// browser runs, the guard's close request, whose reply never reached the agent.
    fn finish(self) {
        self.finish_with(Some(0));
    }

    /// [`Self::finish`], expecting the guard's exit code `code`.
    fn finish_with(self, code: Option<i32>) {
        let (status, rest) = self.guard.finish();
        assert_eq!(status.code(), code);
        assert_eq!(rest, "", "the close reply is swallowed");
        let mut expected = self.forwarded;
        expected.extend(self.started.then(|| {
            json!({
                "jsonrpc": "2.0",
                "id": "clauseo-guard-close",
                "method": "tools/call",
                "params": {"name": "agent_browser_close", "arguments": {}},
            })
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
        "HTTP://Example.com:8/",
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
    // A stopped browser drops notifications, as agent-browser does; a running one gets them.
    session.start_browser();
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

/// Whether the one-shot answering a stopped browser's request exits or the running child does.
#[test]
fn when_the_child_exits_first_the_guard_exits_with_its_code() {
    for running in [false, true] {
        let mut session = Session::start(&format!("filter_child_exits_{running}"));
        if running {
            session.start_browser();
        }
        let exit = json!({"jsonrpc": "2.0", "id": "x", "method": "stub/exit"});
        session.guard.send(&exit);
        // stdin stays open: the guard ends because the child did, and sends no close request.
        // The running child left the call in flight, which the guard answers with an error.
        let rest = session.guard.read_rest();
        if running {
            let answer: Value = serde_json::from_str(rest.trim_end()).unwrap();
            assert_eq!(answer["id"], "x", "{rest}");
            assert_eq!(answer["error"]["code"], -32603, "{rest}");
        } else {
            assert_eq!(rest, "");
        }
        assert_eq!(session.guard.wait().code(), Some(7), "running: {running}");
        let received = session.scratch.received();
        assert_eq!(received.last(), Some(&exit), "running: {running}");
        assert_eq!(received.len(), 1 + usize::from(running));
    }
}

#[test]
fn the_review_rules_end_to_end() {
    let mut session = Session::start("filter_review");
    let screenshot = json!({"jsonrpc": "2.0", "id": "s", "method": "tools/call",
        "params": {"name": "agent_browser_screenshot", "arguments": {"selector": "/Users/x/.zshrc"}}});
    session.guard.send(&screenshot);
    assert_eq!(
        session.guard.read()["result"]["content"][0]["text"],
        "Clauseo does not allow a file path as the screenshot selector in the browser."
    );
    let click = json!({"jsonrpc": "2.0", "id": "c", "method": "tools/call",
        "params": {"name": "agent_browser_click", "arguments": {"selector": "@e1", "newTab": true}}});
    session.guard.send(&click);
    assert_eq!(
        session.guard.read()["result"]["content"][0]["text"],
        refusal("newTab")
    );
    for url in [
        "http://127.0.0.1:41000/",
        "http://0x7f.1/",
        "http://[::1]/",
        "localhost",
    ] {
        assert_eq!(
            session.refused(&json!({ "url": url })),
            refusal("local or private network addresses"),
            "{url}"
        );
    }
    assert_eq!(session.refused(&json!({"webgpu": true})), refusal("webgpu"));
    assert_eq!(
        session.refused(&json!({"timeoutMs": 300_001})),
        refusal("a timeoutMs other than a whole number up to 300000")
    );
    let reserved = json!({"jsonrpc": "2.0", "id": "clauseo-guard-close", "method": "tools/list"});
    session.guard.send(&reserved);
    assert_eq!(session.guard.read()["error"]["code"], -32600);
    let select = json!({"jsonrpc": "2.0", "id": "sel", "method": "tools/call",
        "params": {"name": "agent_browser_select",
            "arguments": {"selector": "#state", "values": ["--Select--", "-Select-", "-5"]}}});
    session.passes(&select);
    let eval = json!({"jsonrpc": "2.0", "id": "ev", "method": "tools/call",
        "params": {"name": "agent_browser_eval", "arguments": {"script": "--x"}}});
    session.passes(&eval);
    assert_eq!(
        session.refused(&json!({"text": "--proxy=x"})),
        refusal("--proxy")
    );
    session.finish();
}

#[test]
fn an_unanswered_close_request_times_out_and_the_guard_still_exits() {
    let mut session = Session::start_with("filter_close_timeout", &["ignore-close"]);
    session.start_browser();
    // Without the close wait's limit the guard would never close the stub's stdin, and the stub
    // would never exit; nextest's slow-timeout would end the test.
    session.finish();
}

#[test]
fn a_child_that_outlives_its_stdin_is_killed() {
    let mut session = Session::start_with("filter_exit_timeout", &["hang"]);
    session.start_browser();
    session.guard.close_stdin();
    assert_eq!(
        session.guard.read_rest(),
        "",
        "the close reply is swallowed"
    );
    assert_eq!(session.guard.wait().code(), Some(1), "killed, so 1");
    assert!(
        session
            .guard
            .stderr_text()
            .contains("did not exit within 5 s of its stdin closing"),
        "the kill is noted on stderr"
    );
    assert_eq!(
        session.scratch.received().len(),
        2,
        "the call, then the close request"
    );
}

#[test]
fn a_line_cut_off_by_the_child_exiting_is_dropped() {
    let mut session = Session::start("filter_partial");
    session
        .guard
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "stub/partial"}));
    assert_eq!(
        session.guard.read_rest(),
        "",
        "half a line never reaches the agent"
    );
    assert_eq!(session.guard.wait().code(), Some(7));
    let stderr = session.guard.stderr_text();
    assert!(stderr.contains("ended in the middle of a line"), "{stderr}");
}

#[test]
fn an_oversize_child_line_kills_the_child_and_fails_the_guard() {
    let mut session = Session::start("filter_child_oversize");
    session
        .guard
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "stub/flood"}));
    assert_eq!(
        session.guard.read_rest(),
        "",
        "nothing of the line reaches the agent"
    );
    assert_eq!(session.guard.wait().code(), Some(1));
    let stderr = session.guard.stderr_text();
    assert!(
        stderr.contains("agent-browser wrote a line over 67108864 bytes; the guard killed it"),
        "{stderr}"
    );
}
