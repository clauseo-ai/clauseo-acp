//! Codex's `app-server` as the fake plays it, and codex-acp's adapter, which answers every
//! request with an error ([`super::Mode::CodexAdapter`]).

use std::io::{self, BufRead};
use std::path::Path;

use serde_json::{Value, json};

use super::{API_KEY_METHOD, EMAIL, marker_method, say};
use crate::EXIT_OK;

/// [`super::Mode::CodexAdapter`]: each JSON-RPC request on stdin gets an error, -32602 for
/// `initialize` and -32601 for any other method, until stdin closes; then exit 0. Codex's
/// `app-server` under `FAKE_SIGNIN=app-server-error` answers so too.
pub(super) fn adapter() -> io::Result<i32> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let code = if request["method"] == "initialize" {
            -32602
        } else {
            -32601
        };
        say(
            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": "refused"}})
                .to_string(),
        )?;
    }
    Ok(EXIT_OK)
}

/// `app-server`: answers each JSON-RPC line on stdin with one line on stdout, until stdin closes.
pub(super) fn app_server(folder: &Path) -> io::Result<i32> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = reply(&line, folder) {
            say(&reply)?;
        }
    }
    Ok(EXIT_OK)
}

/// The reply to one client line, as JSON text, or `None` for a notification (a message with no
/// `id`). `initialize` and `account/read` answer as Codex's schema says; any other method is an
/// error.
fn reply(line: &str, folder: &Path) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {error}")}})
                    .to_string(),
            );
        }
    };
    let id = request.get("id")?.clone();
    let method = request["method"].as_str().unwrap_or_default();
    let reply = match method {
        "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
            "codexHome": folder.to_string_lossy(),
            "platformFamily": if cfg!(windows) { "windows" } else { "unix" },
            "platformOs": std::env::consts::OS,
            "userAgent": "fake_agent codex-cli",
        }}),
        "account/read" => json!({"jsonrpc": "2.0", "id": id, "result": account_read(folder)}),
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32601, "message": format!("method not found: {method}"),
        }}),
    };
    Some(reply.to_string())
}

/// `account/read`'s result. With the marker in `folder`, the `ChatGPT` account
/// `test@example.com` on `plus`, or, when the marker's `authMethod` is `apikey`, an API-key
/// account, which names no email. Without it, the signed-out reply: `account` null and
/// `requiresOpenaiAuth` true.
fn account_read(folder: &Path) -> Value {
    let method = marker_method(folder);
    if method.as_deref() == Some(API_KEY_METHOD) {
        json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": true})
    } else if method.is_some() {
        json!({
            "account": {"type": "chatgpt", "email": EMAIL, "planType": "plus"},
            "requiresOpenaiAuth": false,
        })
    } else {
        json!({"account": null, "requiresOpenaiAuth": true})
    }
}
