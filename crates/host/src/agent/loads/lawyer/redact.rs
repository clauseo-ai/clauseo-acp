//! What the agent's stderr redactor hides for a session's lawyer servers ([`redacted`]): an
//! agent that ends before its session is ready has its last stderr line logged
//! (`start_failure`), and that is when `session/new` held every value and URL below. The
//! redactor matches each string exactly (`clauseo_process`, `stderr/redact.rs`), so each value
//! goes in the forms an error line commonly prints it in.
//!
//! - Each value (a keychain value, env or header, and a plain variable or header value): as it is,
//!   JSON-escaped, percent-encoded (upper- and lowercase hex), the credential after an
//!   `<scheme> ` prefix (`Bearer x`, `Basic x`), and for `Basic` its base64 decoded
//!   (`user:pass`) and the password alone.
//! - Each enabled remote server's URL: whole and percent-decoded; its host (decision record
//!   0047: no host the agent reached in a log), with and without the port, unless it is
//!   `localhost` or `127.0.0.1`; its path with its query; its query; each query value, as it is,
//!   percent-decoded and percent-encoded; each path segment of 16 or more characters, which
//!   looks like a key.
//! - A value under [`MIN`] bytes is not hidden: it would wreck the log wherever those few
//!   characters appear. Such a header or variable value is logged at WARN, naming its server and
//!   name, never the value. A short URL part (`/mcp`, `v1`) is left as it is.

use std::fmt::Write as _;

use base64::Engine as _;
use clauseo_settings::{Effective, McpTransport, McpValue};

use super::McpSecrets;

/// The shortest string the redactor hides (`clauseo_process`'s floor for a named secret).
pub(super) const MIN: usize = 6;
/// The shortest path segment taken as a key.
const KEY_SEGMENT: usize = 16;

/// Every keychain value `secrets` read and every plain header value of the enabled servers, and
/// each enabled remote server's URL, in each form above; sorted, without duplicates, none under
/// [`MIN`] bytes.
pub(crate) fn redacted(effective: &Effective, secrets: &McpSecrets) -> Vec<String> {
    let mut hidden = Vec::new();
    for (account, value) in &secrets.values {
        let mut parts = account.splitn(3, '/');
        let server = parts.next().unwrap_or_default();
        let name = parts.nth(1).unwrap_or_default();
        value_forms(&mut hidden, server, name, value);
    }
    for (server, entry) in effective.enabled_mcp_servers() {
        let values = match &entry.transport {
            McpTransport::Local { env, .. } => env,
            McpTransport::Remote { url, headers } => {
                url_forms(&mut hidden, url);
                headers
            }
        };
        for (name, value) in values {
            if let McpValue::Plain(text) = value {
                value_forms(&mut hidden, server, name, text);
            }
        }
    }
    hidden.retain(|text| text.len() >= MIN);
    hidden.sort_unstable();
    hidden.dedup();
    hidden
}

/// `value`'s forms ([module docs](self)), or a WARN naming `server` and `name` when it is
/// too short to hide.
fn value_forms(hidden: &mut Vec<String>, server: &str, name: &str, value: &str) {
    if value.len() < MIN {
        if !value.is_empty() {
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
    if !host.eq_ignore_ascii_case("localhost") && host != "127.0.0.1" {
        hidden.push(authority.to_owned());
        hidden.push(host.to_owned());
    }
    let Some(tail) = after_scheme.get(end..).filter(|tail| !tail.is_empty()) else {
        return;
    };
    hidden.push(tail.to_owned());
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    for segment in path.split('/') {
        if segment.len() >= KEY_SEGMENT {
            hidden.push(segment.to_owned());
            hidden.extend(percent_decoded(segment));
        }
    }
    if query.is_empty() {
        return;
    }
    hidden.push(query.to_owned());
    for value in query
        .split('&')
        .filter_map(|pair| pair.split_once('=').map(|(_, value)| value))
    {
        hidden.push(value.to_owned());
        if let Some(decoded) = percent_decoded(value) {
            hidden.push(percent_encoded(&decoded));
            hidden.push(decoded);
        } else {
            hidden.push(percent_encoded(value));
        }
    }
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
