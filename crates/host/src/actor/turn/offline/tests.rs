//! The offline classifier on real texts: the systems' own error words on macOS, Linux and
//! Windows, as Codex and Claude Code pass them on, and the server errors that are never offline.

use super::{is_offline, transport_note};

/// Connection-class failures, by system: each is offline.
const OFFLINE: [&str; 21] = [
    // macOS (the stage C sweep's own words, then BSD errno texts)
    "Falling back from WebSockets to HTTPS transport. stream disconnected before completion: \
     Connection refused (os error 61)",
    "stream disconnected before completion: error sending request for url \
     (https://chatgpt.com/backend-api/codex/responses): client error (Connect): tcp connect \
     error: Network is unreachable (os error 51)",
    "error sending request: dns error: failed to lookup address information: nodename nor \
     servname provided, or not known",
    "tcp connect error: Operation timed out (os error 60)",
    "No route to host (os error 65)",
    "Network is down (os error 50)",
    // Linux
    "stream disconnected before completion: Connection refused (os error 111)",
    "dns error: failed to lookup address information: Temporary failure in name resolution",
    "dns error: failed to lookup address information: Name or service not known",
    "tcp connect error: Network is unreachable (os error 101)",
    "tcp connect error: Connection timed out (os error 110)",
    "No route to host (os error 113)",
    // Windows
    "stream disconnected before completion: No connection could be made because the target \
     machine actively refused it. (os error 10061)",
    "dns error: No such host is known. (os error 11001)",
    "A socket operation was attempted to an unreachable network. (os error 10051)",
    "A socket operation was attempted to an unreachable host. (os error 10065)",
    "A connection attempt failed because the connected party did not properly respond after a \
     period of time, or established connection failed because connected host has failed to \
     respond. (os error 10060)",
    // Claude Code, through claude-agent-acp (its line, then the prompt's error)
    "API Error: Connection error.",
    "Internal error: API Error: Connection error.",
    "Unable to connect to API (ECONNREFUSED)",
    "API Error: getaddrinfo ENOTFOUND api.anthropic.com",
];

/// Server errors and other failures: never offline.
const NOT_OFFLINE: [&str; 14] = [
    "unexpected status 503 Service Unavailable: upstream connect error",
    "unexpected status 500 Internal Server Error",
    "exceeded retry limit, last status: 429 Too Many Requests",
    "rate limit exceeded: Rate limit reached for gpt-5 on tokens per min",
    "You've hit your usage limit · resets 3pm",
    "unexpected status 401 Unauthorized: Your refresh token was already used",
    "API Error: 529 Overloaded",
    "API Error: 500 Internal server error. Connection refused by upstream.",
    "We\u{2019}re currently experiencing high demand, which may cause temporary errors.",
    "stream disconnected before completion: websocket closed by server before \
     response.completed",
    "stream disconnected before completion: Transport error: network error: error decoding \
     response body",
    "Quota exceeded. Check your plan and billing details.",
    "Codex ran out of room in the model's context window.",
    "Internal error",
];

#[test]
fn connection_failures_on_every_system_are_offline() {
    for text in OFFLINE {
        assert!(is_offline(text), "not offline: {text}");
    }
}

#[test]
fn server_errors_are_never_offline() {
    for text in NOT_OFFLINE {
        assert!(!is_offline(text), "offline: {text}");
    }
}

#[test]
fn codex_fallback_notes_are_held_back_offline_or_not() {
    assert_eq!(transport_note(OFFLINE[0]), Some(true));
    let closed = "Falling back from WebSockets to HTTPS transport. stream disconnected before \
                  completion: websocket closed by server before response.completed";
    assert_eq!(transport_note(closed), Some(false));
    let windows = "Falling back from WebSockets to HTTPS transport. stream disconnected before \
                   completion: No connection could be made because the target machine actively \
                   refused it. (os error 10061)";
    assert_eq!(transport_note(windows), Some(true));
}

#[test]
fn claude_connection_lines_are_held_back_and_its_server_errors_stay() {
    assert_eq!(transport_note("API Error: Connection error."), Some(true));
    assert_eq!(
        transport_note("Unable to connect to API (ECONNREFUSED)"),
        Some(true)
    );
    assert_eq!(transport_note("API Error: 529 Overloaded"), None);
}

#[test]
fn answer_text_that_quotes_a_failure_stays_in_the_answer() {
    assert_eq!(transport_note("Clause five says the notice period"), None);
    let quoted = "The log says: Connection refused (os error 61). That is the server's port.";
    assert_eq!(transport_note(quoted), None);
    let later = "As the warning put it, Falling back from WebSockets to HTTPS transport.";
    assert_eq!(transport_note(later), None);
}
