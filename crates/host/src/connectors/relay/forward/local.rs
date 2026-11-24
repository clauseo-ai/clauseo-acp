//! The answers the relay gives at once, with no network: `initialize` and `tools/list`, from the
//! copies Clauseo ships (`clauseo_connectors::CASE_LAW_INITIALIZE`, `CASE_LAW_TOOLS_LIST`).
//! Codex lists a turn's tools only from servers already ready, so a server that waited on a slow
//! link for these was left out of new conversations.
//!
//! The protocol version is negotiated here, as MCP's server side does: the agent's requested
//! version when the relay speaks it ([`SUPPORTED_VERSIONS`]), else the newest the relay speaks.
//! The upstream `initialize` (`warm.rs`) asks the server for that same version. The agent's
//! `notifications/initialized` stays here too: the warm-up sends the server its own, after the
//! server's `initialize` answer, as the order requires.

use std::sync::{Arc, LazyLock};

use serde_json::{Value, json};

use super::super::super::texts;
use super::super::conn::Conn;
use super::super::timeline::{self, Mark};
use super::{Call, answer, warm};

/// The MCP versions the relay answers with, newest first: those the case-law server speaks.
pub(super) const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The shipped `initialize` result, parsed once.
static INITIALIZE: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(clauseo_connectors::CASE_LAW_INITIALIZE).unwrap_or_else(|_| json!({}))
});
/// The shipped `tools/list` result, parsed once.
static TOOLS_LIST: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST)
        .unwrap_or_else(|_| json!({"tools": []}))
});

/// The version to answer an agent that asked for `requested`.
pub(super) fn negotiate(requested: Option<&str>) -> &'static str {
    SUPPORTED_VERSIONS
        .iter()
        .find(|version| Some(**version) == requested)
        .or(SUPPORTED_VERSIONS.first())
        .copied()
        .unwrap_or_default()
}

/// The `initialize` result for `version`: the shipped one, with that version and this build's
/// version as the server's.
pub(super) fn initialize_result(version: &str) -> Value {
    let mut result = INITIALIZE.clone();
    if let Value::Object(map) = &mut result {
        map.insert("protocolVersion".to_owned(), json!(version));
        map.insert(
            "serverInfo".to_owned(),
            json!({"name": "clauseo-case-law", "version": env!("CARGO_PKG_VERSION")}),
        );
    }
    result
}

/// The `tools/list` result: the shipped one, one page, whatever cursor was asked for.
pub(super) fn tools_list_result() -> &'static Value {
    &TOOLS_LIST
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
            let first = warm::arm(conn, id.clone(), texts::line(value), version);
            let result = initialize_result(version);
            answer(
                conn,
                &texts::line(&json!({"jsonrpc": "2.0", "id": id, "result": result})),
            );
            timeline::first(session, Mark::Initialize);
            if first {
                warm::start(conn);
            }
            true
        }
        // An `initialize` with no id asks for nothing; the server never hears of it.
        (Some("initialize"), None) => true,
        (Some("tools/list"), Some(id)) => {
            let line = json!({"jsonrpc": "2.0", "id": id, "result": tools_list_result()});
            answer(conn, &texts::line(&line));
            timeline::first(session, Mark::ToolsList);
            true
        }
        (Some("notifications/initialized"), None) => warm::armed(conn),
        _ => false,
    }
}
