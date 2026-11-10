//! What an agent's text says about its connection: whether a server answered
//! ([`says_server`]), whether the connection never got through ([`says_connection`]), and
//! whether a chunk of answer text is the agent's note about its connection ([`transport_note`]).

/// The start of Codex's warning that its WebSocket stream failed (codex-acp forwards each Codex
/// warning as answer text): `Falling back from WebSockets to HTTPS transport. <why>`.
const CODEX_FALLBACK: &str = "falling back from websockets to https transport";

/// How Claude Code writes a failed API call as its assistant message: `API Error: <what>`.
const CLAUDE_PREFIX: &str = "API Error: ";

/// Claude Code's whole connection lines after [`CLAUDE_PREFIX`] (claude-agent-sdk 0.3.293's
/// formatter; "Connection error." from older ones).
const CLAUDE_LINES: [&str; 4] = [
    "Unable to connect to API. Check your internet connection",
    "Request timed out. Check your internet connection and proxy settings",
    "Request timed out.",
    "Connection error.",
];

/// Claude Code's connection lines that end with the error's code in parentheses, as
/// `<start>(<CODE>)`.
const CLAUDE_CODED: [&str; 4] = [
    "Unable to connect to API ",
    "Connection refused \u{2014} a firewall or proxy may be blocking it ",
    "Can't reach the API server \u{2014} check your internet or DNS ",
    "No internet route \u{2014} check your connection or VPN ",
];

/// Words that name a connection that never got through, lowercased: the systems' own error
/// texts (macOS, Linux, Windows), the HTTP stacks' (hyper, reqwest, Node) and the SDKs'.
const CONNECTION: [&str; 35] = [
    "connection refused",
    "actively refused",
    "network is unreachable",
    "unreachable network",
    "unreachable host",
    "no route to host",
    "network is down",
    // Windows' 10050 ("A socket operation encountered a dead network").
    "dead network",
    "failed to lookup address",
    "name or service not known",
    "nodename nor servname",
    "temporary failure in name resolution",
    // Windows' 11002 (WSATRY_AGAIN).
    "temporary error during hostname resolution",
    "no such host is known",
    "could not resolve host",
    "dns error",
    "tcp connect error",
    "operation timed out",
    "connection timed out",
    "request timed out",
    "did not properly respond after a period of time",
    "connection error.",
    "check your internet connection",
    "no internet route",
    "can't reach the api server",
    "econnrefused",
    "connectionrefused",
    "enotfound",
    "eai_again",
    "failedtoopensocket",
    "enetunreach",
    "enetdown",
    "ehostunreach",
    "ehostdown",
    "etimedout",
];

/// Words that say a server answered, lowercased: never offline, whatever else the text says.
const SERVER: [&str; 15] = [
    "unexpected status",
    "status code",
    "http status",
    "rate limit",
    "usage limit",
    "quota",
    "overloaded",
    "unauthorized",
    "forbidden",
    "high demand",
    "internal server error",
    "bad gateway",
    "service unavailable",
    "gateway timeout",
    "too many requests",
];

/// HTTP statuses that, standing as a word of their own, say a server answered.
const STATUSES: [&str; 8] = ["401", "403", "429", "500", "502", "503", "504", "529"];

/// Whether `text` says a server answered: a status, a limit, sign-in, an overload.
pub(in crate::actor::turn) fn says_server(text: &str) -> bool {
    let text = text.to_lowercase();
    SERVER.iter().any(|words| text.contains(words))
        || text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| STATUSES.contains(&word))
}

/// Whether `text` names a connection that never got through: refused, no network or route, no
/// name resolution, a connect or request that timed out.
pub(in crate::actor::turn) fn says_connection(text: &str) -> bool {
    let text = text.to_lowercase();
    CONNECTION.iter().any(|words| text.contains(words))
}

/// Whether `text`, an agent's error or note, says it could not reach its service and does not
/// say a server answered.
pub(in crate::actor::turn) fn is_offline(text: &str) -> bool {
    !says_server(text) && says_connection(text)
}

/// An agent's note about its connection, in its answer text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::actor::turn) struct Note {
    /// Whose words, for the log: `codex_fallback` or `claude_api_error`.
    pub(in crate::actor::turn) source: &'static str,
    /// It names a connection failure.
    pub(in crate::actor::turn) offline: bool,
    /// It is surely the agent's and never answer text: Codex's fallback, a fixed 47-character
    /// start. Claude's line is only suspected (a model could write it), so it is held back,
    /// not dropped.
    pub(in crate::actor::turn) sure: bool,
}

/// The note a chunk of answer text is, or `None` for answer text: a chunk that starts with
/// Codex's fallback warning, or that is, whole, one of Claude Code's connection lines (an
/// answer that quotes one among other words stays as written).
pub(in crate::actor::turn) fn transport_note(text: &str) -> Option<Note> {
    let start = text.trim_start();
    let codex = start
        .get(..CODEX_FALLBACK.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(CODEX_FALLBACK));
    if codex {
        return Some(Note {
            source: "codex_fallback",
            offline: is_offline(text),
            sure: true,
        });
    }
    let line = start.trim_end().strip_prefix(CLAUDE_PREFIX)?;
    let claude = CLAUDE_LINES.contains(&line)
        || CLAUDE_CODED.iter().any(|head| {
            line.strip_prefix(head)
                .and_then(|rest| rest.strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
                .is_some_and(|code| {
                    !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
        });
    (claude && is_offline(line)).then_some(Note {
        source: "claude_api_error",
        offline: true,
        sure: false,
    })
}
