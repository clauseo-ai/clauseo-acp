//! What the agent's stderr redactor hides for a session's lawyer servers ([`redacted`]): an
//! agent that ends before its session is ready has its last stderr line logged
//! (`start_failure`), and that is when `session/new` held every value and URL below. The set is
//! built from the very servers the session is sent (the run's `lawyer::Sent`, one snapshot). The
//! redactor matches each string exactly (`clauseo_process`, `stderr/redact.rs`), so each value
//! goes in the forms an error line commonly prints it in.
//!
//! - Each value, a header's and a local server's variable's (from the keychain or plain, a
//!   firm's policy or a hand-edited file): as it is, JSON-escaped, percent-encoded (upper- and
//!   lowercase hex), the credential after an `<scheme> ` prefix (`Bearer x`, `Basic x`), and for
//!   `Basic` its base64 decoded (`user:pass`) and the password alone. The variables the host
//!   adds itself (`PATH` and the Windows basics, [`HOST_VARS`]) are not secrets and are left.
//! - Each remote server's URL: whole and percent-decoded; its host as written and lower-cased,
//!   with and without the port (decision record 0047: no host the agent reached in a log),
//!   unless it is `localhost` or a loopback address; its path with its query, its path alone,
//!   its query; each path segment and each query value of [`URL_PART`] bytes or more, as it is,
//!   percent-decoded and percent-encoded.
//! - Nothing under [`MIN`] bytes is hidden: it would wreck the log wherever those few characters
//!   appear. A header value that short is logged at WARN, naming its server and header, never
//!   the value; a short variable value (`REGION=uk`) or URL part (`/mcp`, `v1`) is left as
//!   it is.

use std::fmt::Write as _;

use base64::Engine as _;
use clauseo_acp::McpServer;

/// The shortest string the redactor hides (`clauseo_process`'s floor for a named secret).
pub(super) const MIN: usize = 6;
/// The shortest URL host, path segment or query value hidden on its own.
const URL_PART: usize = 8;
/// The variables the host adds to a local server (`lawyer::add_host_env`), by name in any case.
const HOST_VARS: [&str; 6] = ["PATH", "SystemRoot", "windir", "TEMP", "TMP", "PATHEXT"];

/// Every value and URL of `servers` (the lawyer's servers the session is sent), in each form
/// above; sorted, without duplicates, none under [`MIN`] bytes.
pub(crate) fn redacted(servers: &[McpServer]) -> Vec<String> {
    let mut hidden = Vec::new();
    for server in servers {
        match server {
            McpServer::Stdio { name, env, .. } => {
                for (var, value) in env {
                    if !HOST_VARS.iter().any(|host| host.eq_ignore_ascii_case(var)) {
                        value_forms(&mut hidden, name, var, value, false);
                    }
                }
            }
            McpServer::Http { name, url, headers } => {
                url_forms(&mut hidden, url);
                for (header, value) in headers {
                    value_forms(&mut hidden, name, header, value, true);
                }
            }
        }
    }
    hidden.retain(|text| text.len() >= MIN);
    hidden.sort_unstable();
    hidden.dedup();
    hidden
}

/// `value`'s forms ([module docs](self)); when it is too short to hide, nothing, with a WARN
/// naming `server` and `name` if `warn_short` (a header).
fn value_forms(hidden: &mut Vec<String>, server: &str, name: &str, value: &str, warn_short: bool) {
    if value.len() < MIN {
        if warn_short && !value.is_empty() {
            tracing::warn!(
                server,
                name,
                "an MCP server's value is too short for the stderr redactor to hide; it is sent \
                 all the same"
            );
        }
        return;
    }
    text_forms(hidden, value);
    if let Some((scheme, credential)) = value.split_once(' ')
        && !scheme.is_empty()
        && scheme.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        let credential = credential.trim();
        text_forms(hidden, credential);
        if scheme.eq_ignore_ascii_case("basic")
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(credential)
            && let Ok(decoded) = String::from_utf8(bytes)
        {
            if let Some((_, password)) = decoded.split_once(':') {
                text_forms(hidden, password);
            }
            text_forms(hidden, &decoded);
        }
    }
}

/// `text` as it is, JSON-escaped and percent-encoded (upper- and lowercase hex).
fn text_forms(hidden: &mut Vec<String>, text: &str) {
    hidden.push(text.to_owned());
    if let Ok(json) = serde_json::to_string(text)
        && let Some(inner) = json.get(1..json.len() - 1)
    {
        hidden.push(inner.to_owned());
    }
    let encoded = percent_encoded(text);
    hidden.push(encoded.to_ascii_lowercase());
    hidden.push(encoded);
}

/// `url`'s forms ([module docs](self)).
fn url_forms(hidden: &mut Vec<String>, url: &str) {
    hidden.push(url.to_owned());
    hidden.extend(percent_decoded(url));
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let end = after_scheme.find(['/', '?']).unwrap_or(after_scheme.len());
    let authority = after_scheme.get(..end).unwrap_or_default();
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(authority, |(host, _)| host);
    if !loopback(host) {
        for part in [authority, host] {
            if part.len() >= URL_PART {
                hidden.push(part.to_owned());
                hidden.push(part.to_ascii_lowercase());
            }
        }
    }
    let Some(tail) = after_scheme.get(end..).filter(|tail| !tail.is_empty()) else {
        return;
    };
    hidden.push(tail.to_owned());
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    hidden.push(path.to_owned());
    hidden.extend(percent_decoded(path));
    let values = query
        .split('&')
        .filter_map(|pair| pair.split_once('=').map(|(_, value)| value));
    if !query.is_empty() {
        hidden.push(query.to_owned());
    }
    for part in path.split('/').chain(values) {
        if part.len() < URL_PART {
            continue;
        }
        hidden.push(part.to_owned());
        if let Some(decoded) = percent_decoded(part) {
            hidden.push(percent_encoded(&decoded));
            hidden.push(decoded);
        } else {
            hidden.push(percent_encoded(part));
        }
    }
}

/// Whether `host` is this computer: `localhost`, an IPv4 address in `127.0.0.0/8` or `::1`.
fn loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || bare
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `text` with every byte but RFC 3986's unreserved ones (letters, digits, `-._~`) as `%XX`.
fn percent_encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _infallible = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `text` with each `%XX` decoded, when it holds one and the result is UTF-8.
fn percent_decoded(text: &str) -> Option<String> {
    if !text.contains('%') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let hex = bytes
            .get(at + 1..at + 3)
            .filter(|pair| pair.iter().all(u8::is_ascii_hexdigit))
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let (b'%', Some(decoded)) = (byte, hex) {
            out.push(decoded);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    String::from_utf8(out)
        .ok()
        .filter(|decoded| decoded != text)
}
