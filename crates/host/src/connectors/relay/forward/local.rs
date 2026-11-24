//! The answers the relay gives at once, with no network: `initialize`, `tools/list` and `ping`.
//! `initialize` and `tools/list` come from the copy of the server's own answers the hub holds
//! (`copies.rs`: the last the server gave, else the shipped one). Codex lists a turn's tools only
//! from servers already ready, so a server that waited on a slow link for these was left out of
//! new conversations.
//!
//! The protocol version is negotiated here exactly as the server does: the agent's requested
//! version when it is one of [`INITIALIZE_VERSIONS`], else the newest of them. The upstream
//! `initialize` (`warm.rs`) asks the server for that same version, and the copy answered is
//! that version's. The local `initialize` declares the server's own capabilities (`resources`,
//! whose requests are forwarded as before) with `tools.listChanged: true`: when the server's list
//! turns out to differ from the one this connection was given, the agent is sent
//! `notifications/tools/list_changed` and the next `tools/list` gets the server's ([`refresh`]).
//! `tools/list` and `ping` are answered here only once the connection's `initialize` was: an
//! agent that sends none (MCP 2026-07-28 has none) is forwarded as before.
//! The agent's `notifications/initialized` stays here too: the warm-up sends the server its own,
//! after the server's `initialize` answer, as the order requires.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::super::super::texts;
use super::super::conn::Conn;
use super::super::copies::INITIALIZE_VERSIONS;
use super::super::lock;
use super::super::timeline::{self, Mark};
use super::{Call, answer, warm};

/// The tool list a connection answers `tools/list` with, and whether the agent has listed it.
#[derive(Debug)]
pub(crate) struct Listed(Mutex<(Arc<Value>, bool)>);

impl Listed {
    pub(crate) fn new(tools_list: Arc<Value>) -> Listed {
        Listed(Mutex::new((tools_list, false)))
    }
}

/// The version to answer an agent that asked for `requested`.
pub(super) fn negotiate(requested: Option<&str>) -> &'static str {
    INITIALIZE_VERSIONS
        .iter()
        .find(|version| Some(**version) == requested)
        .or(INITIALIZE_VERSIONS.first())
        .copied()
        .unwrap_or_default()
}

/// The `initialize` result for `version` from the copy `initialize`: that version, and the
/// copy's capabilities with `tools.listChanged: true`.
pub(super) fn initialize_result(initialize: &Value, version: &str) -> Value {
    let mut result = initialize.clone();
    if let Value::Object(map) = &mut result {
        map.insert("protocolVersion".to_owned(), json!(version));
        let capabilities = map.entry("capabilities").or_insert_with(|| json!({}));
        if !capabilities.is_object() {
            *capabilities = json!({});
        }
        if let Value::Object(capabilities) = capabilities {
            capabilities.insert("tools".to_owned(), json!({"listChanged": true}));
        }
    }
    result
}

/// Answers `call` here when it is one the relay answers itself; `true` when it did (nothing
/// is forwarded then). `value` is the vetted request: `initialize`'s params are an object.
pub(super) fn answered(conn: &Arc<Conn>, call: &Call, value: &mut Value) -> bool {
    let session = &conn.session;
    match (call.method.as_deref(), call.id.as_ref()) {
        (Some("initialize"), Some(id)) => {
            let requested = value
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str);
            let version = negotiate(requested);
            if let Some(params) = value.get_mut("params").and_then(Value::as_object_mut) {
                params.insert("protocolVersion".to_owned(), json!(version));
            }
            let copy = session.hub.copies.current(version);
            *lock(&conn.listed.0) = (copy.tools_list, false);
            let result = initialize_result(&copy.initialize, version);
            // Armed before the answer: the agent's next line, sent once it reads the answer,
            // must find this connection answered locally.
            warm::arm(conn, id.clone(), texts::line(value), version);
            answer(
                conn,
                &texts::line(&json!({"jsonrpc": "2.0", "id": id, "result": result})),
            );
            timeline::first(session, Mark::Initialize);
            true
        }
        // An `initialize` with no id asks for nothing; the server never hears of it.
        (Some("initialize"), None) => true,
        (Some("tools/list"), Some(id)) if warm::armed(conn) => {
            let tools_list = {
                let mut listed = lock(&conn.listed.0);
                listed.1 = true;
                Arc::clone(&listed.0)
            };
            let line = json!({"jsonrpc": "2.0", "id": id, "result": &*tools_list});
            answer(conn, &texts::line(&line));
            timeline::first(session, Mark::ToolsList);
            true
        }
        (Some("ping"), Some(id)) if warm::armed(conn) => {
            answer(
                conn,
                &texts::line(&json!({"jsonrpc": "2.0", "id": id, "result": {}})),
            );
            true
        }
        (Some("notifications/initialized"), None) => warm::armed(conn),
        _ => false,
    }
}

/// The server's tool list, read by the warm-up: when it is not the one this connection holds,
/// it takes its place, and an agent that had listed the old one is told to list again.
pub(super) fn refresh(conn: &Conn, tools_list: Arc<Value>) {
    let told = {
        let mut listed = lock(&conn.listed.0);
        if *listed.0 == *tools_list {
            return;
        }
        listed.0 = tools_list;
        listed.1
    };
    if told {
        answer(
            conn,
            br#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
        );
    }
    timeline::list_differed(&conn.session, told);
}
