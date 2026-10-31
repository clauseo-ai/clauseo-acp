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
            #[cfg(feature = "e2e")]
            crate::e2e::perf_channel_bytes,
            #[cfg(feature = "marks")]
            crate::marks::marks_memory,
            #[cfg(feature = "marks")]
            crate::marks::marks_finish,
        ])
        .build(context)
        .unwrap();
    #[cfg(feature = "e2e")]
    crate::e2e::grant_scoreboard(&app).unwrap();
    app.manage(bridge);
    let window = WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    (app, window)
}

/// The app's own origin, which the capability's `local` grant matches. The main window sets
/// `use_https_scheme(true)` (`lib.rs`), so `WebView2` serves it from `https://tauri.localhost`;
/// the other webviews serve it from `tauri://localhost`.
#[cfg(windows)]
const LOCAL_ORIGIN: &str = "https://tauri.localhost";
#[cfg(not(windows))]
const LOCAL_ORIGIN: &str = "tauri://localhost";

/// Invokes `cmd` with `body` as the page would.
fn invoke(window: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    invoke_from(window, LOCAL_ORIGIN, cmd, body)
}

/// Invokes `cmd` with `body` from a page at `origin`.
fn invoke_from(
    window: &WebviewWindow<MockRuntime>,
    origin: &str,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.to_owned(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: origin.parse().unwrap(),
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
            json!({ "positions": [{ "stream": "not a stream", "epoch": "x", "cursor": -1 }], "messages": 0 }),
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

    // The same command from a page that is not the app's is refused.
    let foreign = invoke_from(&window, "https://example.com", "display_info", json!({}));
    let denied = foreign
        .as_ref()
        .err()
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("not allowed"));
    assert!(denied, "{foreign:?}");
}

/// The e2e and marks commands answer only in their builds; the default build's capabilities do
/// not grant them, so Tauri refuses them before any handler.
#[test]
fn the_scoreboard_commands_answer_only_in_their_builds() {
    let scratch = Scratch::new("scoreboard-commands", &[]);
    let host = scratch.start(Vec::new());
    let (_app, window) = app(host.bridge());
    let denied = |answer: &Result<Value, Value>| {
        answer
            .as_ref()
            .err()
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("not allowed"))
    };

    let bytes = invoke(&window, "perf_channel_bytes", json!({}));
    if cfg!(feature = "e2e") {
        assert!(bytes.as_ref().is_ok_and(Value::is_u64), "{bytes:?}");
    } else {
        assert!(denied(&bytes), "{bytes:?}");
    }

    let memory = invoke(&window, "marks_memory", json!({}));
    if !cfg!(feature = "marks") {
        assert!(denied(&memory), "{memory:?}");
    } else if cfg!(target_os = "macos") {
        assert!(memory.as_ref().is_ok_and(Value::is_f64), "{memory:?}");
    } else {
        assert_eq!(memory.unwrap_err()["code"], "internal");
    }

    // With MARKS_OUT set (a scoreboard run), the call would quit: only checked without it.
    if std::env::var_os("MARKS_OUT").is_none() {
        let finish = invoke(
            &window,
            "marks_finish",
            json!({ "run": "{}", "error": null }),
        );
        if cfg!(feature = "marks") {
            assert_eq!(finish.unwrap_err()["code"], "command.invalid");
        } else {
            assert!(denied(&finish), "{finish:?}");
        }
    }
}

/// The command names of `list`, a set.
fn set<'a>(list: &[&'a str]) -> BTreeSet<&'a str> {
    let set: BTreeSet<&str> = list.iter().copied().collect();
    assert_eq!(set.len(), list.len(), "{list:?}");
    set
}

/// The names in `build.rs`'s `const <name>: &[&str] = &[...];`.
fn build_list<'a>(build: &'a str, name: &str) -> BTreeSet<&'a str> {
    let start = build.find(&format!("const {name}: &[&str] = &[")).unwrap();
    let end = start + build[start..].find("];").unwrap();
    let list = &build[start..end];
    let list = &list[list.find("= &[").unwrap()..];
    list.split('"').skip(1).step_by(2).collect()
}

/// The permissions `capabilities/<file>` grants, and the window it grants them to.
fn granted(dir: &std::path::Path, file: &str) -> BTreeSet<String> {
    let path = dir.join("capabilities").join(file);
    let capability: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(capability["windows"], json!(["main"]), "{file}");
    capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap().to_owned())
        .collect()
}

/// The `allow-<command>` permissions of `commands`.
fn allowed(commands: &BTreeSet<&str>) -> BTreeSet<String> {
    commands
        .iter()
        .map(|name| format!("allow-{}", name.replace('_', "-")))
        .collect()
}

/// `COMMANDS`, `lib.rs`'s handler, `build.rs`, `capabilities/main.json` and the bindings name the
/// same commands,
/// and the capability grants nothing else. Likewise `E2E_COMMANDS` and `MARKS_COMMANDS` with
/// `build.rs`'s `E2E` and `MARKS`, `capabilities/e2e.json` and `marks.json`, and the handler's
/// entries under `#[cfg(feature = "e2e")]` and `#[cfg(feature = "marks")]`.
#[test]
fn every_list_of_commands_agrees() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = set(&COMMANDS);
    let e2e = set(&E2E_COMMANDS);
    let marks = set(&MARKS_COMMANDS);

    let build = std::fs::read_to_string(dir.join("build.rs")).unwrap();
    assert_eq!(build_list(&build, "RELEASE"), ours, "build.rs");
    assert_eq!(build_list(&build, "E2E"), e2e, "build.rs");
    assert_eq!(build_list(&build, "MARKS"), marks, "build.rs");

    assert_eq!(
        granted(dir, "main.json"),
        allowed(&ours),
        "capabilities/main.json"
    );
    assert_eq!(
        granted(dir, "e2e.json"),
        allowed(&e2e),
        "capabilities/e2e.json"
    );
    assert_eq!(
        granted(dir, "marks.json"),
        allowed(&marks),
        "capabilities/marks.json"
    );
    // The default build names only main; e2e::grant_scoreboard adds the other two in their builds.
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("tauri.conf.json")).unwrap())
            .unwrap();
    assert_eq!(config["app"]["security"]["capabilities"], json!(["main"]));

    let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
    let start = lib.find("generate_handler![").unwrap() + "generate_handler![".len();
    let end = start + lib[start..].find("])").unwrap();
    let mut handled: [BTreeSet<&str>; 3] = Default::default();
    for entry in lib[start..end]
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        let (feature, path) = match entry.rsplit_once(char::is_whitespace) {
            Some((attribute, path)) => (Some(attribute.trim()), path),
            None => (None, entry),
        };
        let (place, module) = match feature {
            None => (0, "commands::"),
            Some(r#"#[cfg(feature = "e2e")]"#) => (1, "e2e::"),
            Some(r#"#[cfg(feature = "marks")]"#) => (2, "marks::"),
            Some(other) => panic!("lib.rs: {other} {path}"),
        };
        let name = path
            .strip_prefix(module)
            .unwrap_or_else(|| panic!("lib.rs: {entry}"));
        handled[place].insert(name);
    }
    assert_eq!(handled, [ours.clone(), e2e, marks], "lib.rs");

    let bindings =
        std::fs::read_to_string(dir.join("../../ui/app/src/transport/bindings.ts")).unwrap();
    let invoked: BTreeSet<&str> = bindings
        .split("__TAURI_INVOKE(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(invoked, ours, "bindings.ts");
}
