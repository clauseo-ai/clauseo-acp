//! An RPC error's `data` as the driver keeps it for classifying (`RpcData`): only the
//! allowlisted short tokens, never the lawyer's words; and `Error::prompt_too_long` on it.

use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use crate::Error;
use crate::error::{MAX_FIELD_BYTES, RpcData, data_text};

/// The lawyer's question, as an adapter's error data might echo it.
const PROMPT: &str = "Draft the Zanzibar indemnity clause for Quarrington Holdings";

fn rpc(message: &str, data: &Value) -> Error {
    Error::Rpc {
        method: "session/prompt".to_owned(),
        code: -32603,
        message: message.to_owned(),
        data: String::new(),
        data_text: data_text(Some(data)),
        data_fields: RpcData::read(Some(data)),
    }
}

#[test]
fn rpc_data_keeps_only_allowlisted_tokens() {
    let echoing = json!({
        "errorKind": "prompt_too_long",
        "terminal_reason": PROMPT,
        "codex_error_info": {"badRequest": {"input": PROMPT}},
        "message": PROMPT,
        "prompt": [{"type": "text", "text": PROMPT}],
    });
    let kept = RpcData::read(Some(&echoing));
    assert_eq!(kept.error_kind(), Some("prompt_too_long"));
    assert_eq!(
        kept.terminal_reason(),
        None,
        "a value with spaces is not a token"
    );
    assert_eq!(kept.codex_error_info(), None, "an object is not a token");
    for word in ["Zanzibar", "Quarrington", "indemnity"] {
        assert!(!format!("{kept:?}").contains(word), "{kept:?}");
    }
    // A token over the bound, or with anything but letters, digits, `_`, `-` and `.`, is dropped.
    let long = "a".repeat(MAX_FIELD_BYTES + 1);
    let edge = "b".repeat(MAX_FIELD_BYTES);
    let read = |value: &str| RpcData::read(Some(&json!({"errorKind": value})));
    assert_eq!(read(&long).error_kind(), None);
    assert_eq!(read(&edge).error_kind(), Some(edge.as_str()));
    for bad in ["", "too/long", "prompt\ntoo", "é"] {
        assert_eq!(read(bad).error_kind(), None, "{bad:?}");
    }
    // Codex's app-server spelling, and none for no data or a string.
    let camel = RpcData::read(Some(&json!({"codexErrorInfo": "contextWindowExceeded"})));
    assert_eq!(camel.codex_error_info(), Some("contextWindowExceeded"));
    assert_eq!(RpcData::read(None), RpcData::default());
    assert_eq!(RpcData::read(Some(&json!(PROMPT))), RpcData::default());
}

#[test]
fn prompt_too_long_reads_structure_first_then_codex_s_exact_text() {
    let none = json!(null);
    let too_long = [
        rpc("Internal error", &json!({"errorKind": "prompt_too_long"})),
        rpc(
            "Internal error",
            &json!({"terminal_reason": "prompt_too_long"}),
        ),
        rpc(
            "Internal error",
            &json!({"codex_error_info": "context_window_exceeded"}),
        ),
        rpc(
            "Internal error",
            &json!({"codexErrorInfo": "contextWindowExceeded"}),
        ),
        rpc(
            "Internal error",
            &json!({"message": "Input exceeds the maximum length of 1048576 characters."}),
        ),
        rpc(
            "Input exceeds the maximum length of 1048576 characters.",
            &none,
        ),
    ];
    for error in too_long {
        assert!(error.prompt_too_long(), "{error:?}");
        // The code stays the RPC error's: only the host's turn end reads `prompt_too_long`.
        assert_eq!(error.code(), ErrorCode::AgentProtocolError);
    }
    let other = [
        rpc("Internal error", &json!({"errorKind": "invalid_request"})),
        rpc("Internal error", &json!({"errorKind": "max_output_tokens"})),
        rpc("Internal error", &none),
        rpc(
            "Error: Input exceeds the maximum length of 1048576 characters.",
            &none,
        ),
        rpc("Internal error", &json!({"message": "Prompt is too long"})),
        rpc("Internal error", &json!({"other": "prompt_too_long"})),
    ];
    for error in other {
        assert!(!error.prompt_too_long(), "{error:?}");
    }
    assert!(!Error::Closed.prompt_too_long());
    assert!(!Error::Protocol("prompt_too_long".to_owned()).prompt_too_long());
}
