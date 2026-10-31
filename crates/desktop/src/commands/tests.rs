//! The commands through Tauri's own IPC, on the mock runtime: what Tauri rejects before a command
//! runs, what a command refuses, and the one list of command names.

use std::collections::BTreeSet;

use clauseo_protocol::{AgentChoiceId, HostCommand, PermissionRequestId};
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{Manager as _, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use super::*;
use crate::test_support::{Scratch, envelope};

/// An app on the mock runtime with every command, the app's own context (so its capabilities)
/// and a bridge, and its `main` window.
fn app(bridge: Bridge) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let context = crate::context::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            dispatch,
            query,
            open_stream,
            subscribe,
            unsubscribe,
            ack,
            answer_permission,
            pick_folder,
            display_info,
        ])
        .build(context)
        .unwrap();
    app.manage(bridge);
    let window = WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    (app, window)
}

/// Invokes `cmd` with `body` as the page would.
fn invoke(window: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.to_owned(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: tauri::http::HeaderMap::default(),
        invoke_key: INVOKE_KEY.to_owned(),
    };
    get_ipc_response(window, request).map(|body| match body {
        InvokeResponseBody::Json(text) => serde_json::from_str(&text).unwrap(),
        InvokeResponseBody::Raw(_) => panic!("a raw answer"),
    })
}

/// Tauri rejects an argument that does not decode before the command runs, with a plain string,
/// not a `HostError`: the webview's transport maps such a rejection to `internal`. A command
/// that runs answers every refusal with a `HostError` object.
#[test]
fn a_tauri_rejection_is_a_string_and_a_refusal_is_a_host_error() {
    let scratch = Scratch::new("tauri-rejection", &[]);
    let host = scratch.start(Vec::new());
    let (_app, window) = app(host.bridge());

    for (cmd, body) in [
        ("dispatch", json!({ "command": 42 })),
        ("dispatch", json!({})),
        (
            "ack",
            json!({ "stream": "not a stream", "epoch": "x", "cursor": -1, "frames": 0 }),
        ),
        (
            "query",
            json!({ "query": { "page_entries": { "limit": 100_000 } } }),
        ),
    ] {
        let rejection = invoke(&window, cmd, body.clone()).unwrap_err();
        let text = rejection.as_str().unwrap_or_default();
        let decode = text.contains("invalid args") || text.contains("missing required key");
        assert!(decode, "{cmd} {body}: {rejection}");
    }
    // A command no capability grants, the dialog plugin's included, is a string too.
    for cmd in ["plugin:dialog|open", "no_such_command"] {
        let rejection = invoke(&window, cmd, json!({})).unwrap_err();
        let denied = rejection
            .as_str()
            .is_some_and(|text| text.contains("not allowed"));
        assert!(denied, "{cmd}: {rejection}");
    }

    let thread = host.thread();
    let answer = envelope(HostCommand::AnswerPermission {
        thread,
        request: PermissionRequestId::new(Uuid::now_v7()).unwrap(),
        option_id: AgentChoiceId::new("allow").unwrap(),
    });
    let refusal = invoke(&window, "dispatch", json!({ "command": answer })).unwrap_err();
    assert_eq!(refusal["code"], "command.invalid", "{refusal}");
    assert_eq!(refusal["retryable"], false, "{refusal}");
    assert!(refusal["message"].is_string(), "{refusal}");

    let info = invoke(&window, "display_info", json!({})).unwrap();
    let platform = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    };
    assert_eq!(info, json!({ "platform": platform }));
}

/// `COMMANDS`, `lib.rs`'s handler, `build.rs`, `capabilities/main.json` and the bindings name the
/// same commands,
/// and the capability grants nothing else.
#[test]
fn every_list_of_commands_agrees() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours: BTreeSet<&str> = COMMANDS.iter().copied().collect();
    assert_eq!(ours.len(), COMMANDS.len());

    let build = std::fs::read_to_string(dir.join("build.rs")).unwrap();
    let start = build.find(".commands(&[").unwrap();
    let end = start + build[start..].find("])").unwrap();
    let listed: BTreeSet<&str> = build[start..end].split('"').skip(1).step_by(2).collect();
    assert_eq!(listed, ours, "build.rs");

    let capability: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("capabilities/main.json")).unwrap())
            .unwrap();
    let granted: BTreeSet<String> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap().to_owned())
        .collect();
    let expected: BTreeSet<String> = COMMANDS
        .iter()
        .map(|name| format!("allow-{}", name.replace('_', "-")))
        .collect();
    assert_eq!(granted, expected, "capabilities/main.json");

    let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    let start = lib.find("generate_handler![").unwrap();
    let end = start + lib[start..].find(']').unwrap();
    let handled: BTreeSet<&str> = lib[start..end]
        .split("commands::")
        .skip(1)
        .map(|rest| rest.trim_end_matches(|c: char| c == ',' || c.is_whitespace()))
        .collect();
    assert_eq!(handled, ours, "lib.rs");

    let bindings =
        std::fs::read_to_string(dir.join("../../ui/app/src/transport/bindings.ts")).unwrap();
    let invoked: BTreeSet<&str> = bindings
        .split("__TAURI_INVOKE(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(invoked, ours, "bindings.ts");
}
