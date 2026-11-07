//! Codex's `app-server` as the fake plays it, and codex-acp's adapter, which answers every
//! request with an error ([`super::Mode::CodexAdapter`]).

use std::io::{self, BufRead};
use std::path::Path;

use serde_json::{Value, json};

use super::{
    ACCOUNT_READ_LOG, API_KEY_METHOD, Marker, NO_ACCOUNT_METHOD, NO_EMAIL_METHOD,
    REFRESH_REFUSED_METHOD, REFRESH_SILENT_METHOD, marker, marker_path, say, write,
};
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
        match reply(&line, folder) {
            Reply::Line(reply) => say(&reply)?,
            Reply::Nothing => {}
            // Closes before answering: the client reads no answer.
            Reply::Close => return Ok(EXIT_OK),
        }
    }
    Ok(EXIT_OK)
}

/// What the app-server does with one line.
enum Reply {
    /// Answers this JSON text.
    Line(String),
    /// Answers nothing (a notification).
    Nothing,
    /// Closes without answering ([`REFRESH_SILENT_METHOD`]'s forced refresh).
    Close,
}

/// `account/read` with `params`: they are appended to [`ACCOUNT_READ_LOG`]; with
/// `refreshToken: true`, the marker's [`REFRESH_REFUSED_METHOD`] refuses it and
/// [`REFRESH_SILENT_METHOD`] closes, else the marker file is rewritten in place (same file,
/// same content, a new last write), as Codex's refresh rewrites `auth.json`, then the read
/// answers as without it.
fn account_read_reply(id: &Value, params: &Value, folder: &Path) -> Reply {
    let mut log = params.to_string();
    log.push('\n');
    drop(write::append(folder.join(ACCOUNT_READ_LOG), &log));
    if params["refreshToken"] == Value::Bool(true) {
        let method = marker(folder).map(|marker| marker.method);
        if method.as_deref() == Some(REFRESH_SILENT_METHOD) {
            return Reply::Close;
        }
        if method.as_deref() == Some(REFRESH_REFUSED_METHOD) {
            return Reply::Line(
                json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32603, "message": "refresh_token_reused",
                }})
                .to_string(),
            );
        }
        if let Some(path) = marker_path(folder) {
            let text = std::fs::read(&path).unwrap_or_default();
            drop(write::write_in_place(&path, &text));
        }
    }
    Reply::Line(json!({"jsonrpc": "2.0", "id": id, "result": account_read(folder)}).to_string())
}

/// The reply to one client line: JSON text, nothing for a notification (a message with no
/// `id`), or a close. `initialize` and `account/read` answer as Codex's schema says; any other
/// method is an error.
fn reply(line: &str, folder: &Path) -> Reply {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Reply::Line(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {error}")}})
                    .to_string(),
            );
        }
    };
    let Some(id) = request.get("id").cloned() else {
        return Reply::Nothing;
    };
    let method = request["method"].as_str().unwrap_or_default();
    let reply = match method {
        "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
            "codexHome": folder.to_string_lossy(),
            "platformFamily": if cfg!(windows) { "windows" } else { "unix" },
            "platformOs": std::env::consts::OS,
            "userAgent": "fake_agent codex-cli",
        }}),
        "account/read" => return account_read_reply(&id, &request["params"], folder),
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32601, "message": format!("method not found: {method}"),
        }}),
    };
    Reply::Line(reply.to_string())
}

/// `account/read`'s result. With the marker in `folder`, the `ChatGPT` account
/// named by the marker's email (`test@example.com` unless a test planted another, as R17's swap
/// tests do) on `plus`; when the marker's `authMethod` is `apikey`, an API-key
/// account, which names no email; when it is `chatgpt-no-email`, a `ChatGPT` account whose email
/// is `null`. Without it, or when the marker's `authMethod` is `no-account` (`login status` then
/// still says signed in), the signed-out reply: `account` null and `requiresOpenaiAuth` true.
fn account_read(folder: &Path) -> Value {
    let marker = marker(folder);
    let method = marker.as_ref().map(|marker| marker.method.as_str());
    if method == Some(NO_ACCOUNT_METHOD) {
        json!({"account": null, "requiresOpenaiAuth": true})
    } else if method == Some(API_KEY_METHOD) {
        json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": true})
    } else if method == Some(NO_EMAIL_METHOD) {
        json!({
            "account": {"type": "chatgpt", "email": null, "planType": "plus"},
            "requiresOpenaiAuth": false,
        })
    } else if let Some(Marker { email, .. }) = &marker {
        json!({
            "account": {"type": "chatgpt", "email": email, "planType": "plus"},
            "requiresOpenaiAuth": false,
        })
    } else {
        json!({"account": null, "requiresOpenaiAuth": true})
    }
}
