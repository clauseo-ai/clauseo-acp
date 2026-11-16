//! The pass list, `allowedDomains` in every spelling, the advertised schemas and the
//! domain-restriction error, read against agent-browser v0.41.1's own `tools/list` reply for its
//! `core` tools (`agent_browser_core_tools.json`: the binary's reply to `mcp --tools core`, its
//! descriptions taken out), and the caps on the keys that hold a time.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::super::domains::{
    ALLOWED_DOMAINS, CDP_REFUSAL, RESTARTED, domain_restricted, names_allowed_domains, replacement,
};
use super::super::keys::{COMMON_KEYS, TOOL_KEYS, UNSUPPORTED, advertised, passes};
use super::super::{
    ALLOWED_TOOLS, EXTRA_ARGS_ALLOWED, MUST_BE_FALSE, READ_FETCH_KEYS, REFUSED_KEYS, TIME_KEYS,
    advertised_tools, time_max, time_refusal,
};
use super::{refused_on, verdict};
use crate::browser_guard::filter::Verdict;

const CORE_TOOLS: &str = include_str!("agent_browser_core_tools.json");

/// Keys of agent-browser's schemas the pass list leaves off without a refusal by name: each is
/// refused as not supported (`keys.rs` says why).
const LEFT_OFF: &[(&str, &str)] = &[
    ("agent_browser_open", "webmcp"),
    ("agent_browser_close", "all"),
];

fn core_tools() -> Vec<Value> {
    let reply: Value = serde_json::from_str(CORE_TOOLS).unwrap();
    reply["result"]["tools"].as_array().unwrap().clone()
}

fn properties(tool: &Value) -> &Map<String, Value> {
    tool["inputSchema"]["properties"].as_object().unwrap()
}

/// A value of the key's schema type that no other check refuses.
fn harmless(key: &str, schema: &Value) -> Value {
    match key {
        "url" => return json!("https://example.com"),
        "extraArgs" => return json!(["--json"]),
        "selector" => return json!("#a"),
        _ => {}
    }
    match schema["type"].as_str() {
        Some("boolean") => json!(false),
        Some("integer") => json!(1),
        Some("number") => json!(0.5),
        Some("array") => json!(["x"]),
        _ => json!("x"),
    }
}

/// Every key of every allowed tool's schema is either on the pass list, refused by name
/// (`REFUSED_KEYS`, `allowedDomains`, the read tool's fetch keys) or left off on purpose
/// ([`LEFT_OFF`]), never two of these; each passes or is refused as classified; and every key on
/// the pass list is one agent-browser declares. A version bump that adds a key fails here.
#[test]
fn every_key_of_agent_browsers_schemas_is_classified() {
    let tools = core_tools();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let mut offered: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| *n != "agent_browser_eval")
        .collect();
    offered.sort_unstable();
    let mut allowed = ALLOWED_TOOLS.to_vec();
    allowed.sort_unstable();
    assert_eq!(offered, allowed, "the core profile less eval");
    let listed: Vec<&str> = TOOL_KEYS.iter().map(|(tool, _)| *tool).collect();
    assert_eq!(
        listed, ALLOWED_TOOLS,
        "one pass list per allowed tool, in order"
    );
    for tool in tools.iter().filter(|t| t["name"] != "agent_browser_eval") {
        let name = tool["name"].as_str().unwrap();
        let declared = properties(tool);
        for (key, schema) in declared {
            let by_name = REFUSED_KEYS.contains(&key.as_str())
                || key == "allowedDomains"
                || (name == "agent_browser_read" && READ_FETCH_KEYS.contains(&key.as_str()));
            let left_off = LEFT_OFF.contains(&(name, key.as_str()));
            let classes = [passes(name, key), by_name, left_off];
            assert_eq!(
                classes.iter().filter(|c| **c).count(),
                1,
                "{name}.{key}: {classes:?}"
            );
            let arguments = json!({ key.as_str(): harmless(key, schema) });
            let forwarded = refused_on(name, &arguments).is_none();
            assert_eq!(forwarded, passes(name, key), "{name}.{key}");
        }
        for key in COMMON_KEYS
            .iter()
            .chain(TOOL_KEYS.iter().find(|(t, _)| *t == name).unwrap().1)
        {
            assert!(declared.contains_key(*key), "{name}.{key} is not declared");
        }
    }
}

/// `allowedDomains` gets its own refusal in every spelling, as a key at any depth or a flag in
/// any string, `extraArgs` included, on any tool; the words alone, not as a flag, are text.
#[test]
fn allowed_domains_is_refused_in_every_spelling() {
    for arguments in [
        json!({"allowedDomains": ["sci.gov.in", "www.sci.gov.in"]}),
        json!({"allowedDomains": []}),
        json!({"allowedDomains": null}),
        json!({"allowed_domains": ["sci.gov.in"]}),
        json!({"allowed-domains": "sci.gov.in"}),
        json!({"--allowed-domains": "sci.gov.in"}),
        json!({"AllowedDomains": "sci.gov.in"}),
        json!({"ALLOWED_DOMAINS": "sci.gov.in"}),
        json!({"options": {"allowedDomains": ["sci.gov.in"]}}),
        json!({"extraArgs": ["--allowed-domains", "sci.gov.in"]}),
        json!({"extraArgs": ["--allowed-domains=sci.gov.in"]}),
        json!({"url": "https://sci.gov.in", "text": "--allowed_domains=sci.gov.in"}),
    ] {
        for tool in [
            "agent_browser_open",
            "agent_browser_snapshot",
            "agent_browser_close",
        ] {
            assert_eq!(
                refused_on(tool, &arguments).as_deref(),
                Some(ALLOWED_DOMAINS),
                "{tool} {arguments}"
            );
        }
        assert!(names_allowed_domains(arguments.as_object().unwrap()));
    }
    assert_eq!(
        ALLOWED_DOMAINS,
        "Clauseo already controls which sites the browser may open; open the page without \
         allowedDomains."
    );
    let text = json!({"selector": "#q", "text": "allowedDomains allowed-domains"});
    assert_eq!(refused_on("agent_browser_fill", &text), None);
}

/// A key off the pass list is refused with one fixed line, never passed on, and its text never
/// comes back.
#[test]
fn an_unknown_key_is_not_supported() {
    assert_eq!(UNSUPPORTED, "This option is not supported in Clauseo.");
    for (tool, arguments) in [
        ("agent_browser_open", json!({"bogus": 1})),
        // Another tool's key.
        ("agent_browser_open", json!({"selector": "#a"})),
        ("agent_browser_open", json!({ "k".repeat(100): 1 })),
        ("agent_browser_open", json!({"<script>": 1})),
    ] {
        assert_eq!(
            refused_on(tool, &arguments),
            Some(UNSUPPORTED.to_owned()),
            "{arguments}"
        );
    }
    for (tool, key) in LEFT_OFF {
        assert_eq!(
            refused_on(tool, &json!({ *key: false })),
            Some(UNSUPPORTED.to_owned())
        );
    }
    let notification = json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "agent_browser_open", "arguments": {"bogus": 1}}});
    assert_eq!(verdict(&notification), Verdict::Drop);
}

/// The advertised `tools/list` names no refused tool and no refused key: each kept tool's
/// properties are exactly the declared keys that pass, less the keys that pass only as
/// `false`; `extraArgs` lists its tokens, `timeoutMs` its maximum, and `required` only keys
/// still there. `allowedDomains` appears nowhere in it.
#[test]
fn the_tool_list_offers_only_what_passes() {
    let original = core_tools();
    let line = advertised_tools(CORE_TOOLS.as_bytes()).unwrap();
    let text = String::from_utf8(line.clone()).unwrap();
    for word in [
        "allowedDomains",
        "allowed-domains",
        "agent_browser_eval",
        "webmcp",
    ] {
        assert!(!text.contains(word), "{word}");
    }
    let reply: Value = serde_json::from_slice(&line).unwrap();
    let tools = reply["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), ALLOWED_TOOLS.len());
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let before = original.iter().find(|t| t["name"] == name).unwrap();
        let expected: BTreeSet<&str> = properties(before)
            .keys()
            .map(String::as_str)
            .filter(|key| advertised(name, key))
            .collect();
        let kept: BTreeSet<&str> = properties(tool).keys().map(String::as_str).collect();
        assert_eq!(kept, expected, "{name}");
        for key in &kept {
            assert!(
                passes(name, key) && !MUST_BE_FALSE.contains(key),
                "{name}.{key}"
            );
        }
        let props = properties(tool);
        assert_eq!(
            props["extraArgs"]["items"]["enum"],
            json!(EXTRA_ARGS_ALLOWED)
        );
        for key in TIME_KEYS {
            if let Some(time) = props.get(*key) {
                let max = if *key == "delayMs" { 1000 } else { 300_000 };
                assert_eq!(time["maximum"], max, "{name}.{key}");
            }
        }
        assert!(props.contains_key("timeoutMs"), "{name}");
        if name == "agent_browser_type" {
            assert_eq!(props["delayMs"]["maximum"], 1000);
        }
        for key in tool["inputSchema"]["required"]
            .as_array()
            .into_iter()
            .flatten()
        {
            assert!(
                props.contains_key(key.as_str().unwrap()),
                "{name} requires {key}"
            );
        }
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    let open = tools
        .iter()
        .find(|t| t["name"] == "agent_browser_open")
        .unwrap();
    let open_keys: Vec<&String> = properties(open).keys().collect();
    assert_eq!(open_keys, ["extraArgs", "timeoutMs", "url"]);
}

/// `agent_browser_read` is offered as reading the open tab: its description and title no
/// longer offer the `url` the guard refuses. Other tools' texts stay as agent-browser wrote them.
#[test]
fn the_read_tool_no_longer_offers_a_url() {
    let reply = json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [
        {"name": "agent_browser_read", "title": "Read URL",
            "description": "Fetch a URL as agent-readable text, preferring text/markdown. Omit url \
                to read the active tab.",
            "inputSchema": {"type": "object", "properties": {"url": {"type": "string"},
                "raw": {"type": "boolean"}}}},
        {"name": "agent_browser_open", "title": "Open page", "description": "Navigate to a URL.",
            "inputSchema": {"type": "object", "properties": {"url": {"type": "string"}}}},
    ]}});
    let line = advertised_tools(reply.to_string().as_bytes()).unwrap();
    let tools: Value = serde_json::from_slice(&line).unwrap();
    let read = &tools["result"]["tools"][0];
    assert_eq!(
        read["description"],
        "Read the open tab as agent-readable text."
    );
    assert_eq!(read["title"], "Read page");
    assert_eq!(
        read["inputSchema"]["properties"],
        json!({"raw": {"type": "boolean"}})
    );
    assert_eq!(
        tools["result"]["tools"][1]["description"],
        "Navigate to a URL."
    );
}

/// The reply of a stuck agent-browser, as the release binary sends it: the CLI's error in
/// `structuredContent.response.error`, its JSON in the text.
fn stuck_reply(error: &str) -> Value {
    let inner = json!({"success": false, "error": error}).to_string();
    json!({"jsonrpc": "2.0", "id": 4, "result": {
        "content": [{"type": "text", "text": error}], "isError": true,
        "structuredContent": {"exitCode": 1, "response": {"error": error, "success": false},
            "stderr": "", "stdout": inner}}})
}

/// agent-browser's refusal of `--allowed-domains` counts only as its own error field, whole: a
/// page's thrown error or title quoting it does not, nor any other error. The replacement keeps
/// the request's id.
#[test]
fn the_domain_restriction_error_is_recognised() {
    assert!(domain_restricted(
        stuck_reply(CDP_REFUSAL).to_string().as_bytes()
    ));
    let as_error =
        json!({"jsonrpc": "2.0", "id": 4, "error": {"code": -32603, "message": CDP_REFUSAL}});
    assert!(domain_restricted(as_error.to_string().as_bytes()));
    // A page whose title getter throws the sentence: agent-browser's "Evaluation error: ...".
    let evaluation = format!("Evaluation error: Error: {CDP_REFUSAL}");
    assert!(!domain_restricted(
        stuck_reply(&evaluation).to_string().as_bytes()
    ));
    // The sentence as page text in a result, an error or not.
    for is_error in [true, false] {
        let quoted = json!({"jsonrpc": "2.0", "id": 4, "result": {
            "content": [{"type": "text", "text": CDP_REFUSAL}], "isError": is_error,
            "structuredContent": {"response": {"success": true,
                "data": {"title": CDP_REFUSAL}}}}});
        assert!(!domain_restricted(quoted.to_string().as_bytes()));
    }
    let other = stuck_reply("net::ERR_NAME_NOT_RESOLVED");
    assert!(!domain_restricted(other.to_string().as_bytes()));
    assert!(!domain_restricted(b"not json"));
    let mut cut = stuck_reply(CDP_REFUSAL).to_string().into_bytes();
    cut.truncate(cut.len() - 2);
    assert!(!domain_restricted(&cut));
    let reply: Value = serde_json::from_slice(&replacement(&json!("x"), RESTARTED)).unwrap();
    assert_eq!(reply["id"], "x");
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(reply["result"]["content"][0]["text"], RESTARTED);
}

/// `timeoutMs`, and every other time in milliseconds, is capped at five minutes; `delayMs`,
/// waited between every two keystrokes, at one second.
#[test]
fn times_are_capped() {
    assert_eq!(
        time_refusal("timeoutMs"),
        "a timeoutMs other than a whole number up to 300000"
    );
    assert_eq!(
        time_refusal("delayMs"),
        "a delayMs other than a whole number up to 1000"
    );
    for key in TIME_KEYS {
        let max = time_max(key);
        assert_eq!(max, if *key == "delayMs" { 1000 } else { 300_000 }, "{key}");
        for good in [json!(1), json!(max / 2), json!(max)] {
            assert_eq!(
                refused_on(tool_of(key), &json!({ *key: good })),
                None,
                "{key} {good}"
            );
        }
        for bad in [
            json!(max + 1),
            json!(u64::MAX),
            json!(1e12),
            json!(-1),
            json!("5"),
            json!(1.5),
        ] {
            assert_eq!(
                refused_on(tool_of(key), &json!({ *key: bad })),
                Some(time_refusal(key)),
                "{key} {bad}"
            );
        }
    }
}

/// A tool that takes `key`.
fn tool_of(key: &str) -> &'static str {
    match key {
        "waitTimeoutMs" => "agent_browser_wait_for_text",
        "ms" => "agent_browser_wait_ms",
        "delayMs" => "agent_browser_type",
        "readTimeoutMs" => "agent_browser_read",
        _ => "agent_browser_open",
    }
}
