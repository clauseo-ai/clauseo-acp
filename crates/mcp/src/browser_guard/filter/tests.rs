use serde_json::{Value, json};

use super::{
    BATCH_MESSAGE, EXTRA_ARGS_ALLOWED, MUST_BE_FALSE, REFUSED_KEYS, SCREENSHOT_PATH, TIMEOUT,
    Verdict, check, refusal_text,
};

fn call_tool(tool: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    })
}

fn verdict(message: &Value) -> Verdict {
    check(message.to_string().as_bytes())
}

/// The refusal's name, or `None` when the line is forwarded unchanged.
fn refused(arguments: &Value) -> Option<String> {
    refused_on("agent_browser_open", arguments)
}

fn refused_on(tool: &str, arguments: &Value) -> Option<String> {
    let message = call_tool(tool, arguments);
    match verdict(&message) {
        Verdict::Forward(text) => {
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), message);
            None
        }
        Verdict::Answer(text) => {
            let answer: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(answer["id"], 7);
            assert_eq!(answer["result"]["isError"], true);
            let words = answer["result"]["content"][0]["text"].as_str().unwrap();
            let name = words
                .strip_prefix("Clauseo does not allow ")
                .and_then(|rest| rest.strip_suffix(" in the browser."))
                .unwrap();
            Some(name.to_owned())
        }
        Verdict::Drop => panic!("a request is never dropped"),
    }
}

#[test]
fn every_refused_key_is_refused_whatever_its_value() {
    let keys = [
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
    ];
    assert_eq!(
        REFUSED_KEYS.len(),
        keys.len(),
        "the literal list is the whole list"
    );
    for key in keys {
        for value in [json!("x"), json!(false), json!(null), json!(0)] {
            assert_eq!(
                refused(&json!({ *key: value })).as_deref(),
                Some(key),
                "{key}"
            );
        }
    }
}

#[test]
fn keys_match_exactly() {
    for near in [
        "Session",
        "session ",
        "idle_timeout",
        "PATH",
        "screenshot_dir",
        "Headed",
    ] {
        assert_eq!(refused(&json!({ near: "x" })), None, "{near}");
    }
}

#[test]
fn urls_pass_only_for_the_web_and_about_blank() {
    for good in [
        "https://example.com",
        "  HTTPS://Example.com",
        "about:blank",
        "ABOUT:BLANK",
        "example.com/path",
        "sci.gov.in",
    ] {
        assert_eq!(refused(&json!({ "url": good })), None, "{good}");
    }
    for (bad, name) in [
        ("file:///etc/passwd", "file URLs"),
        ("FILE:///etc/passwd", "file URLs"),
        (" javascript:alert(1)", "javascript URLs"),
        ("data:text/html,x", "data URLs"),
        ("chrome://settings", "chrome URLs"),
        ("chrome-extension://abc/x", "chrome-extension URLs"),
        ("view-source:https://a", "view-source URLs"),
        ("devtools://devtools/x", "devtools URLs"),
        ("about:settings", "about URLs"),
        ("about:blank#x", "about URLs"),
        ("http:/example.com", "http URLs"),
        ("localhost:8080", "localhost URLs"),
        ("http://127.0.0.1:8/x", "local or private network addresses"),
        (
            "http://[::1]:9222/json",
            "local or private network addresses",
        ),
        ("2130706433", "local or private network addresses"),
        ("http://0x7f.1/", "local or private network addresses"),
        (
            "http://evil@192.168.0.1/",
            "local or private network addresses",
        ),
        (
            "http://%31%32%37.0.0.1/",
            "URLs whose host Clauseo cannot read",
        ),
        (
            "http://bücher.de/",
            "non-ASCII host names (use the xn-- form)",
        ),
        ("java\tscript:alert(1)", "non-web URLs"),
        ("\u{feff}https://a", "non-web URLs"),
    ] {
        assert_eq!(
            refused(&json!({ "url": bad })).as_deref(),
            Some(name),
            "{bad:?}"
        );
    }
    assert_eq!(refused(&json!({ "url": 5 })).as_deref(), Some("url"));
    assert_eq!(
        refused(&json!({ "url": ["https://a"] })).as_deref(),
        Some("url")
    );
}

#[test]
fn extra_args_take_only_the_short_list() {
    for token in EXTRA_ARGS_ALLOWED {
        assert_eq!(refused(&json!({ "extraArgs": [token] })), None, "{token}");
    }
    assert_eq!(
        refused(&json!({ "extraArgs": ["--json", "-i", "--full"] })),
        None
    );
    for (tokens, name) in [
        (json!(["--headed"]), "--headed"),
        (json!(["--engine=lightpanda"]), "--engine"),
        (json!(["--proxy", "http://evil:1"]), "--proxy"),
        (json!(["--json", "evil"]), "evil"),
        (json!(["--json=false"]), "--json"),
        (json!(["--JSON"]), "--JSON"),
        (json!(["-s"]), "-s"),
        (json!([""]), ""),
        (json!(["--json", 1]), "extraArgs"),
        (json!(["--json", null]), "extraArgs"),
        (json!("--json"), "extraArgs"),
        (json!({"0": "--json"}), "extraArgs"),
    ] {
        assert_eq!(
            refused(&json!({ "extraArgs": tokens })).as_deref(),
            Some(name),
            "{tokens}"
        );
    }
}

#[test]
fn typed_strings_that_read_as_flags_are_refused() {
    for (arguments, name) in [
        (json!({"selector": "--headed"}), "--headed"),
        (json!({"url": "--session"}), "--session"),
        (json!({"text": "--restore=saved"}), "--restore"),
        (json!({"values": ["ok", "-p"]}), "-p"),
        (json!({"allowedDomains": ["--engine"]}), "--engine"),
        (
            json!({"deep": {"x": ["--screenshot-dir"]}}),
            "--screenshot-dir",
        ),
    ] {
        assert_eq!(refused(&arguments).as_deref(), Some(name), "{arguments}");
    }
    for fine in [
        "-",
        "--",
        "-5",
        "-10",
        "---",
        "---x",
        "- item",
        "a --headed",
        "",
        "−−headed",
        "--Select--",
        "-Select-",
        "-a_b",
        "-é",
    ] {
        assert_eq!(refused(&json!({ "text": fine })), None, "{fine:?}");
        assert_eq!(refused(&json!({ "values": [fine] })), None, "{fine:?}");
    }
    for (bad, name) in [
        ("--headed", "--headed"),
        ("-p", "-p"),
        ("--proxy=x", "--proxy"),
        ("--proxy=", "--proxy"),
        ("-Rohan", "-Rohan"),
        ("--a1-b2", "--a1-b2"),
    ] {
        assert_eq!(
            refused(&json!({ "text": bad })).as_deref(),
            Some(name),
            "{bad}"
        );
    }
}

#[test]
fn eval_script_alone_may_look_like_a_flag() {
    for script in ["--x", "-p", "--headed"] {
        assert_eq!(
            refused_on("agent_browser_eval", &json!({ "script": script })),
            None
        );
        assert_eq!(
            refused_on("agent_browser_eval", &json!({ "text": script })).as_deref(),
            Some(script)
        );
        assert_eq!(
            refused_on("agent_browser_fill", &json!({ "script": script })).as_deref(),
            Some(script)
        );
    }
}

#[test]
fn new_tab_and_webgpu_pass_only_as_false() {
    assert_eq!(MUST_BE_FALSE, ["newTab", "webgpu"]);
    for key in ["newTab", "webgpu"] {
        assert_eq!(refused(&json!({ key: false })), None, "{key}");
        for value in [json!(true), json!(null), json!("false"), json!(0)] {
            assert_eq!(
                refused(&json!({ key: value })).as_deref(),
                Some(key),
                "{key}"
            );
        }
    }
    assert_eq!(
        refused_on(
            "agent_browser_click",
            &json!({"selector": "@e1", "newTab": true})
        )
        .as_deref(),
        Some("newTab")
    );
}

#[test]
fn timeout_is_capped_at_five_minutes() {
    for good in [json!(1), json!(30_000), json!(300_000)] {
        assert_eq!(refused(&json!({ "timeoutMs": good })), None, "{good}");
    }
    for bad in [
        json!(300_001),
        json!(u64::MAX),
        json!(1e12),
        json!(-1),
        json!("5"),
        json!(1.5),
    ] {
        assert_eq!(
            refused(&json!({ "timeoutMs": bad })).as_deref(),
            Some(TIMEOUT),
            "{bad}"
        );
    }
}

#[test]
fn a_screenshot_selector_that_reads_as_a_path_is_refused() {
    let tool = "agent_browser_screenshot";
    for path in [
        "/Users/x/.zshrc",
        "./a",
        "../a",
        ".a/b",
        "#a/b",
        "@e1/x",
        "a\\b",
        "C:\\Users\\x",
        "x.png",
        "x.PNG",
        ".x.jpg",
        "#x.Jpeg",
        "x.webp",
    ] {
        assert_eq!(
            refused_on(tool, &json!({ "selector": path })).as_deref(),
            Some(SCREENSHOT_PATH),
            "{path}"
        );
    }
    for selector in [
        "#main",
        ".card",
        "@e3",
        "div > p",
        "img[alt=logo]",
        "x.pngx",
    ] {
        assert_eq!(
            refused_on(tool, &json!({ "selector": selector })),
            None,
            "{selector}"
        );
    }
    assert_eq!(
        refusal_text(SCREENSHOT_PATH),
        "Clauseo does not allow a file path as the screenshot selector in the browser."
    );
    // Other tools' selectors are not positional paths.
    assert_eq!(
        refused_on("agent_browser_click", &json!({"selector": "a/b"})),
        None
    );
}

#[test]
fn the_reserved_id_is_refused_on_any_message() {
    for message in [
        json!({"jsonrpc": "2.0", "id": "clauseo-guard-close", "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": "clauseo-guard-close", "method": "tools/call",
            "params": {"name": "agent_browser_close", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": "clauseo-guard-close", "result": {}}),
    ] {
        let Verdict::Answer(text) = verdict(&message) else {
            panic!("{message}")
        };
        let answer: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(answer["id"], "clauseo-guard-close");
        assert_eq!(answer["error"]["code"], -32600);
    }
    let near = json!({"jsonrpc": "2.0", "id": "clauseo-guard-close2", "method": "tools/list"});
    assert!(matches!(verdict(&near), Verdict::Forward(_)));
}

#[test]
fn long_names_are_cut() {
    let long = format!("--{}", "x".repeat(100));
    let name = refused(&json!({ "extraArgs": [long] })).unwrap();
    assert_eq!(name, format!("--{}...", "x".repeat(62)));
}

#[test]
fn ordinary_calls_pass() {
    for arguments in [
        json!({}),
        json!({"url": "https://sci.gov.in", "allowedDomains": ["sci.gov.in"], "timeoutMs": 5}),
        json!({"selector": "#q", "text": "Section 138 -- cheque"}),
        json!({"key": "-"}),
    ] {
        assert_eq!(refused(&arguments), None, "{arguments}");
    }
}

#[test]
fn arguments_that_are_not_an_object_pass_on() {
    for arguments in [json!(null), json!("--headed"), json!(["--headed"])] {
        assert_eq!(refused(&arguments), None, "{arguments}");
    }
    let no_arguments = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {}});
    assert!(matches!(verdict(&no_arguments), Verdict::Forward(_)));
}

#[test]
fn only_tools_call_is_checked() {
    for method in ["tools/list", "initialize", "Tools/Call", "tools/call "] {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": method,
            "params": {"arguments": {"session": "x"}}});
        assert!(matches!(verdict(&message), Verdict::Forward(_)), "{method}");
    }
}

#[test]
fn a_refused_notification_is_dropped() {
    let message = json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"headed": true}}});
    assert_eq!(verdict(&message), Verdict::Drop);
}

#[test]
fn an_id_of_null_or_a_string_comes_back_as_given() {
    for id in [json!(null), json!("abc"), json!(1.5)] {
        let message = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "x", "arguments": {"headed": true}}});
        let Verdict::Answer(text) = verdict(&message) else {
            panic!()
        };
        let answer: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(answer["id"], id);
        assert_eq!(
            answer["result"]["content"][0]["text"],
            refusal_text("headed")
        );
    }
}

#[test]
fn duplicate_keys_are_read_as_agent_browser_reads_them() {
    let last_bad = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agent_browser_open","arguments":{"url":"https://a","url":"file:///etc/passwd"}}}"#;
    assert!(matches!(check(last_bad), Verdict::Answer(_)));
    let last_good = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agent_browser_open","arguments":{"url":"file:///etc/passwd","url":"https://a"}}}"#;
    let Verdict::Forward(text) = check(last_good) else {
        panic!()
    };
    assert!(
        !text.contains("file:"),
        "only the value checked is forwarded: {text}"
    );
}

#[test]
fn unreadable_lines_batches_and_scalars_are_answered() {
    let parse = r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#;
    for bad in [&b""[..], b"{", b"not json", b"{\"a\":1}x", b"\xff\xfe"] {
        let Verdict::Answer(text) = check(bad) else {
            panic!()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::from_str::<Value>(parse).unwrap()
        );
    }
    let Verdict::Answer(text) = check(br#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#)
    else {
        panic!()
    };
    let answer: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(answer["error"]["code"], -32600);
    assert_eq!(answer["error"]["message"], BATCH_MESSAGE);
    for scalar in [&b"5"[..], b"\"x\"", b"null"] {
        let Verdict::Answer(text) = check(scalar) else {
            panic!()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap()["error"]["code"],
            -32600
        );
    }
}

#[test]
fn a_forwarded_line_is_one_line() {
    let message = br##"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"agent_browser_fill","arguments":{"selector":"#a","text":"line\nnext\u2028"}}}"##;
    let Verdict::Forward(text) = check(message) else {
        panic!()
    };
    assert!(!text.contains('\n') && !text.contains('\r'));
}

#[test]
fn the_decision_record_lists_the_same_rules() {
    let record = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/decisions/0029-browser.md"
    ))
    .unwrap();
    let listed = |words: &[&str]| {
        let names: Vec<String> = words.iter().map(|word| format!("`{word}`")).collect();
        format!("{}.", names.join(", "))
    };
    let keys = format!("- **Refused keys:** {}", listed(REFUSED_KEYS));
    let must_be_false = format!("- **Refused unless `false`:** {}", listed(MUST_BE_FALSE));
    let tokens = format!(
        "- **`extraArgs` tokens allowed:** {}",
        listed(EXTRA_ARGS_ALLOWED)
    );
    for line in [keys, must_be_false, tokens] {
        assert!(
            record.lines().any(|l| l.trim_start() == line),
            "0029-browser.md lacks: {line}"
        );
    }
}
