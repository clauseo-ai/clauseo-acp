//! What the thread log keeps of the web: the caps on a search's query and results and on a page's
//! title, and the one rule for every URL the log keeps, [`logged_url`] (0041).
//!
//! Producers build [`WebSearchRecorded`](crate::WebSearchRecorded) and
//! [`PageVisited`](crate::PageVisited) with their `new` constructors, which apply the caps and the
//! URL rule. `protocol` has no I/O and no URL parser in its dependencies, so the rule parses by
//! hand.

/// Most characters (`char`s, Unicode scalar values) in a search query. A longer query is cut to
/// this, at a character boundary, by `WebSearchRecorded::new`.
pub const MAX_SEARCH_QUERY_CHARS: usize = 1_000;

/// Most characters in a result's title or in a visited page's title. A longer title is cut to
/// this, at a character boundary, by `WebResult::new` and `PageVisited::new`.
pub const MAX_WEB_TITLE_CHARS: usize = 300;

/// Most characters in a logged URL, after [`logged_url`] has run. A longer URL is cut to this, at
/// a character boundary, by `WebResult::new` and `PageVisited::new`.
pub const MAX_URL_CHARS: usize = 4_096;

/// Most results a search records. `WebSearchRecorded::new` keeps the first this many, in the
/// order the agent got them.
pub const MAX_SEARCH_RESULTS: usize = 50;

/// What a redacted query parameter's value becomes.
pub const REDACTED: &str = "REDACTED";

/// Query parameter names whose values are redacted, compared case-insensitively. A name ending in
/// `*` matches every name that starts with the rest of it. Lowercase, so the comparison is a
/// plain one.
const SECRET_NAMES: [&str; 21] = [
    "token",
    "access_token",
    "id_token",
    "key",
    "apikey",
    "api_key",
    "sig",
    "signature",
    "auth",
    "password",
    "pass",
    "session",
    "sid",
    "code",
    "state",
    "x-amz-*",
    "x-goog-*",
    "expires",
    "awsaccesskeyid",
    "policy",
    "key-pair-id",
];

/// Cuts `text` to at most `max` characters, at a character boundary: a longer one is cut, a
/// shorter one is returned as it is.
pub(crate) fn cut_chars(mut text: String, max: usize) -> String {
    if let Some((end, _)) = text.char_indices().nth(max) {
        text.truncate(end);
    }
    text
}

/// The rule for every URL the log keeps (0041), a pure function of the text.
///
/// For an absolute `http` or `https` URL, it returns the URL with:
/// - its fragment (from the first `#`) dropped;
/// - its userinfo (`user:pass@`) dropped;
/// - the value of each query parameter whose name is in [`SECRET_NAMES`] (compared
///   case-insensitively after percent-decoding the name) replaced by [`REDACTED`];
/// - the value of each query parameter whose value is 32 or more characters, all hex or all
///   base64url (`A-Z a-z 0-9 - _`, with up to two `=` padding at the end), replaced by
///   [`REDACTED`].
///
/// Everything else stays byte for byte: the scheme, host, port, path, the query's other
/// parameters with their names, percent-encoding and order. A query parameter with no `=` has no
/// value, so it is kept, unless the whole of it is a long hex or base64url value, which becomes
/// [`REDACTED`].
///
/// A string that is not an absolute `http` or `https` URL is kept with only its fragment dropped.
///
/// The result is not capped; the constructors cut it to [`MAX_URL_CHARS`] after this runs, so a
/// secret past the cut is redacted before it is removed.
pub fn logged_url(raw: &str) -> String {
    let without_fragment = raw.split_once('#').map_or(raw, |(before, _)| before);
    let Some((scheme, authority, path, query)) = split_http(without_fragment) else {
        return without_fragment.to_owned();
    };
    let mut out = format!("{scheme}://{authority}{path}");
    if let Some(query) = query {
        out.push('?');
        let params: Vec<String> = query.split('&').map(redact_param).collect();
        out.push_str(&params.join("&"));
    }
    out
}

/// Splits an absolute `http` or `https` URL, which has no fragment, into its scheme, its
/// authority without userinfo, its path, and its query (without the `?`) when it has one.
/// `None` when it is not such a URL: no `://`, another scheme, or an empty host.
fn split_http(url: &str) -> Option<(&str, &str, &str, Option<&str>)> {
    let (scheme, after) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let (authority, rest) = match after.find(['/', '?']) {
        Some(end) => after.split_at(end),
        None => (after, ""),
    };
    // The userinfo ends at the last `@` of the authority; the host is what follows it.
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.is_empty() {
        return None;
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    Some((scheme, host, path, query))
}

/// One query parameter, redacted or as it was.
fn redact_param(param: &str) -> String {
    match param.split_once('=') {
        Some((name, _)) if is_secret_name(name) => format!("{name}={REDACTED}"),
        Some((name, value)) if is_secret_value(value) => format!("{name}={REDACTED}"),
        None if is_secret_value(param) => REDACTED.to_owned(),
        _ => param.to_owned(),
    }
}

/// Whether a parameter name is one of [`SECRET_NAMES`], after percent-decoding and lowercasing.
fn is_secret_name(name: &str) -> bool {
    let name = percent_decode_lower(name);
    SECRET_NAMES.iter().any(|secret| {
        secret
            .strip_suffix('*')
            .map_or_else(|| name == *secret, |prefix| name.starts_with(prefix))
    })
}

/// Whether a value is 32 or more characters, all hex or all base64url with up to two `=` padding
/// characters at its end.
fn is_secret_value(value: &str) -> bool {
    if value.len() < 32 {
        return false;
    }
    if value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    let body = value.trim_end_matches('=');
    value.len() - body.len() <= 2 && !body.is_empty() && body.bytes().all(is_base64url)
}

fn is_base64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// The name with each `%XX` escape decoded and every byte lowercased, for comparing names. A
/// malformed escape is kept as its characters.
fn percent_decode_lower(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let escaped = (b == b'%')
            .then(|| {
                let high = bytes.get(i + 1).and_then(|&c| hex_value(c))?;
                let low = bytes.get(i + 2).and_then(|&c| hex_value(c))?;
                Some(high * 16 + low)
            })
            .flatten();
        if let Some(decoded) = escaped {
            out.push(decoded.to_ascii_lowercase());
            i += 3;
        } else {
            out.push(b.to_ascii_lowercase());
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
