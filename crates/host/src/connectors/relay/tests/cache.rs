//! The copy of the server's `initialize` and `tools/list` the relay answers from: the shipped
//! one on a first start, then the server's (sent to the agent as `list_changed`), cached per
//! origin so the next start answers with it at once; a broken or oversized cache file falls back
//! to the shipped copy and is replaced.

use serde_json::{Value, json};

use super::super::copies::MAX_CACHE_BYTES;
use super::support::*;

/// The shipped `initialize` result's instructions, with the rig's base URL, as the server
/// would write them.
fn shipped_instructions(rig: &Rig) -> String {
    let shipped: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_INITIALIZE).unwrap();
    shipped["instructions"]
        .as_str()
        .unwrap()
        .replace("https://data.clauseo.chat", &rig.origin.prefix())
}

/// A server whose text changed since the build: new instructions, a changed description and a
/// new tool.
fn changed_server(rig: &Rig) -> (Value, Value) {
    let mut initialize: Value =
        serde_json::from_str(clauseo_connectors::CASE_LAW_INITIALIZE).unwrap();
    initialize["instructions"] =
        json!("Every word of a query must appear. Use both citator routes.");
    initialize["serverInfo"] = json!({"name": "clauseo-case-law", "version": "9.9.9"});
    let mut tools: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    tools["tools"][0]["description"] =
        json!("Search Indian law; every word of a query must appear.");
    tools["tools"].as_array_mut().unwrap().push(
        json!({"name": "new_tool", "description": "New.", "inputSchema": {"type": "object"}}),
    );
    rig.fake.set_mcp_lists(initialize.clone(), tools.clone());
    (initialize, tools)
}

/// The cache file, parsed, once it holds `tools_list`.
fn cached_once(rig: &Rig, tools_list: &Value) -> Value {
    let file = rig.cache_file();
    let mut cached = Value::Null;
    wait_until("the cache file", || {
        cached = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        cached["tools_list"] == *tools_list
    });
    cached
}

#[test]
fn a_first_start_serves_the_shipped_copy_then_the_servers_list_and_caches_it() {
    let rig = rig();
    rig.sign_in();
    let (initialize, tools) = changed_server(&rig);
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = rig.agent();
    let (init, list, _took) = ready_answers(&mut agent);
    assert_eq!(init["result"]["instructions"], shipped_instructions(&rig));
    let shipped: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    assert_eq!(list["result"], shipped);
    held.cancel();
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
    );
    agent.send_value(&request(3, "tools/list"));
    let fresh = agent.recv();
    assert_eq!(fresh["result"], tools);
    assert!(names(&fresh).contains(&"new_tool".to_owned()));
    let cached = cached_once(&rig, &tools);
    assert_eq!(cached["initialize"], initialize);
    // The warm-up asked for the list in this session's version.
    let list_request = rig
        .mcp_requests()
        .into_iter()
        .find(|request| request.json().unwrap()["method"] == "tools/list")
        .unwrap();
    assert_eq!(
        list_request.header("mcp-protocol-version"),
        Some("2025-06-18")
    );
    assert!(cached["fetched_at"].is_i64(), "{cached}");
    // The two results only: no session id, credential or header.
    let text = std::fs::read_to_string(rig.cache_file()).unwrap();
    assert!(!text.contains("fake-session-0001"), "{text}");
    let bearer = rig.handle.bearer().unwrap().0.expose().to_owned();
    assert!(!text.contains(&bearer));
    assert!(!text.contains(rig.token.expose()));
}

#[test]
fn the_next_start_serves_the_cached_copy_at_once_instructions_included() {
    let rig = rig();
    rig.sign_in();
    let (initialize, tools) = changed_server(&rig);
    let mut first = rig.agent();
    first.send_value(&initialize_request());
    first.recv();
    cached_once(&rig, &tools);
    // A restart: new relays on the same data folder, the server held.
    let relays = rig.relays_again();
    let (session, token) = relays.open(key(2), loopback()).unwrap();
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = Agent::connect(&session);
    assert_eq!(
        agent.hello(token.expose(), "case-law"),
        Some(json!({"clauseo_relay": 1, "ok": true}))
    );
    let (init, list, took) = ready_answers(&mut agent);
    assert_eq!(init["result"]["instructions"], initialize["instructions"]);
    assert_eq!(init["result"]["serverInfo"]["version"], "9.9.9");
    assert_eq!(list["result"], tools);
    eprintln!("with a cache: ready in {} ms", took.as_millis());
    held.cancel();
}

/// `initialize` with id 1.
fn initialize_request() -> Value {
    initialize(1)
}

/// A rig whose cache file starts as `bytes`: the shipped copy is served, and the warm-up
/// replaces the file with the server's.
fn falls_back_and_is_replaced(bytes: Vec<u8>) {
    let rig = rig_with(&RigOptions {
        cache: Some(bytes),
        ..RigOptions::default()
    });
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = rig.agent();
    let (init, list, _took) = ready_answers(&mut agent);
    assert_eq!(init["result"]["instructions"], shipped_instructions(&rig));
    let shipped: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    assert_eq!(list["result"], shipped);
    held.cancel();
    let file = rig.cache_file();
    wait_until("the cache file replaced", || {
        std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|cached| cached["tools_list"] == shipped && cached["fetched_at"] != 1)
    });
    assert!(std::fs::metadata(&file).unwrap().len() < 64 * 1024);
}

#[test]
fn a_corrupt_cache_falls_back_to_the_shipped_copy_and_is_replaced() {
    falls_back_and_is_replaced(b"{not json".to_vec());
}

#[test]
fn an_oversized_cache_falls_back_to_the_shipped_copy_and_is_replaced() {
    let mut initialize: Value =
        serde_json::from_str(clauseo_connectors::CASE_LAW_INITIALIZE).unwrap();
    initialize["instructions"] = json!("x".repeat(MAX_CACHE_BYTES));
    let tools: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    let big = json!({"initialize": initialize, "tools_list": tools, "fetched_at": 1});
    falls_back_and_is_replaced(serde_json::to_vec(&big).unwrap());
}

#[test]
fn an_empty_cached_list_falls_back_to_the_shipped_copy() {
    let empty = json!({"initialize": {"instructions": "x"}, "tools_list": {"tools": []}});
    falls_back_and_is_replaced(serde_json::to_vec(&empty).unwrap());
}

#[test]
fn another_version_does_not_get_the_cached_copy() {
    let rig = rig();
    rig.sign_in();
    let (_initialize, tools) = changed_server(&rig);
    let mut first = rig.agent();
    first.send_value(&initialize_request());
    first.recv();
    cached_once(&rig, &tools);
    // A 2025-03-26 agent on the same relays: the shipped copy of that version, not 2025-06-18's.
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = rig.agent();
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {}}}));
    let init = agent.recv();
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["instructions"], shipped_instructions(&rig));
    assert!(init["result"]["serverInfo"].get("title").is_none());
    agent.send_value(&request(2, "tools/list"));
    let shipped: Value =
        serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST_2025_03_26).unwrap();
    assert_eq!(agent.recv()["result"], shipped);
    held.cancel();
}
