//! The identity check's one JSON-RPC error mapping, on Codex 0.162.1's exact strings.

use super::{AccountRead, Checked, rpc_error_kind};

#[test]
fn only_codexs_unauthorized_routing_error_is_a_refusal() {
    // codex-rs 0.162.1, workspace_routing.rs:64: OpenAI refused the login's tokens.
    assert_eq!(
        rpc_error_kind(-32603, "workspace routing discovery unauthorized (401)"),
        Checked::Refused
    );
    let no_answer = Checked::Read(AccountRead::NoAnswer);
    // workspace_routing.rs:62: what an offline app-server gives, forced or not.
    assert_eq!(
        rpc_error_kind(-32603, "workspace routing discovery failed"),
        no_answer
    );
    // Any other error tells nothing either, the same words under another code included.
    assert_eq!(rpc_error_kind(-32603, "refresh_token_reused"), no_answer);
    assert_eq!(
        rpc_error_kind(-32602, "workspace routing discovery unauthorized (401)"),
        no_answer
    );
    assert_eq!(rpc_error_kind(-32601, "method not found"), no_answer);
}
