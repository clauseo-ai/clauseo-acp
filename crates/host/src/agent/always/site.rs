//! Sites (`always.rs`): a `WebFetch` URL and a Claude shell's `SandboxNetworkAccess` host read
//! as one exact host name, the same kept entry, `WebFetch(domain:<host>)`.

use super::well_formed;

/// The host of an `http` or `https` URL, lower case, for a `WebFetch(domain:…)` rule; none for
/// another scheme, no host, or a host that is not a plain name or IPv4 address.
pub(super) fn domain(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    domain_name(&host).then_some(host)
}

/// Letters, digits, `.` and `-` only, not starting with `.` or `-` nor ending in `.` (`a.com.`
/// is the same site as `a.com`, and would be a second entry).
pub(super) fn domain_name(host: &str) -> bool {
    !host.starts_with(['.', '-'])
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-'))
}

/// A Claude `SandboxNetworkAccess` card's `rawInput.host` as a site: not empty, none of
/// `/ @ : ? # \`, a space or a control character, not ending in `.`; then read as WHATWG reads
/// `http://<host>/` (lower case, an international name in punycode), as [`domain`] reads a URL.
pub(super) fn sandbox_host(host: &str) -> Option<String> {
    let odd = |c: char| matches!(c, '/' | '@' | ':' | '?' | '#' | '\\') || c.is_whitespace();
    let refused =
        host.is_empty() || host.ends_with('.') || host.chars().any(|c| odd(c) || c.is_control());
    if refused {
        return None;
    }
    domain(&format!("http://{host}/"))
}

/// The hosts of the well-formed `WebFetch(domain:…)` rules among `rules`, in order: the sites
/// the matter keeps, for both Claude's `WebFetch` and its shell's network.
pub(crate) fn kept_sites(rules: &[String]) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| well_formed(rule))
        .filter_map(|rule| rule.strip_prefix("WebFetch(domain:")?.strip_suffix(')'))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests;
