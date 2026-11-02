//! Reading a request's target: `host:port` for CONNECT, an `http://` URL otherwise, and how
//! its body is framed. No network, no lists: what the checks start from.

use std::net::IpAddr;

use crate::HostRefusal;
use crate::proxy::allowlist::Name;
use crate::proxy::head::Request;
use crate::proxy::relay::Framing;

/// A request the proxy will carry, once its checks have passed.
pub(crate) struct Target {
    pub(crate) name: Name,
    pub(crate) port: u16,
    pub(crate) kind: Kind,
}

pub(crate) enum Kind {
    Connect,
    Http { url: url::Url, framing: Framing },
}

/// A request refused before its checks: the status, and the denial's reason if it gets one.
pub(crate) struct Refusal {
    pub(crate) status: u16,
    pub(crate) why: Option<HostRefusal>,
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl Refusal {
    fn bad() -> Refusal {
        Refusal {
            status: 400,
            why: None,
            host: String::new(),
            port: 0,
        }
    }

    fn scheme(host: String, port: u16) -> Refusal {
        Refusal {
            status: 405,
            why: Some(HostRefusal::Scheme),
            host,
            port,
        }
    }
}

/// Reads the target: `host:port` for CONNECT, an `http://` URL otherwise.
pub(crate) fn classify(request: &Request) -> Result<Target, Refusal> {
    if request.method == "CONNECT" {
        let (name, port) = authority(&request.target).ok_or_else(Refusal::bad)?;
        return Ok(Target {
            name,
            port,
            kind: Kind::Connect,
        });
    }
    let framing = framing(request).ok_or_else(Refusal::bad)?;
    if request.target.starts_with('/') || request.target == "*" {
        let host = request.values("host");
        let (host, port) = match host.as_slice() {
            [one] => std::str::from_utf8(one)
                .ok()
                .and_then(|h| host_and_port(h, 80))
                .map_or((String::new(), 0), |(name, port)| (name.text(), port)),
            _ => (String::new(), 0),
        };
        return Err(Refusal::scheme(host, port));
    }
    let url = url::Url::parse(&request.target).map_err(|error| {
        tracing::debug!(%error, "egress: request target is not a URL");
        Refusal::bad()
    })?;
    let name = match url.host() {
        Some(url::Host::Domain(domain)) => Name::parse(domain),
        Some(url::Host::Ipv4(ip)) => Some(Name::Ip(IpAddr::V4(ip))),
        Some(url::Host::Ipv6(ip)) => Some(Name::Ip(IpAddr::V6(ip))),
        None => None,
    };
    let port = url.port_or_known_default().unwrap_or(0);
    if url.scheme() != "http" {
        return Err(Refusal::scheme(
            name.map(|n| n.text()).unwrap_or_default(),
            port,
        ));
    }
    let name = name.ok_or_else(Refusal::bad)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refusal::bad());
    }
    Ok(Target {
        name,
        port,
        kind: Kind::Http { url, framing },
    })
}

/// `host:port` with a port of digits only; an IPv6 host in brackets.
fn authority(target: &str) -> Option<(Name, u16)> {
    if target.contains(['/', '@', '?', '#']) {
        return None;
    }
    let (host, port) = target.rsplit_once(':')?;
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        return None;
    }
    let port = digits(port)?;
    Some((Name::parse(host)?, port))
}

/// A `Host` header's host and port (`default` when it has none).
fn host_and_port(value: &str, default: u16) -> Option<(Name, u16)> {
    match value.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => Some((Name::parse(host)?, digits(port)?)),
        Some(_) | None => Some((Name::parse(value)?, default)),
    }
}

fn digits(port: &str) -> Option<u16> {
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    port.parse().ok()
}

/// How the request's body ends; `None` (400) for both `Content-Length` and `Transfer-Encoding`,
/// lengths that disagree or are not digits, or a transfer coding other than `chunked`.
fn framing(request: &Request) -> Option<Framing> {
    let lengths = request.values("content-length");
    let codings = request.values("transfer-encoding");
    match (lengths.as_slice(), codings.as_slice()) {
        ([], []) => Some(Framing::Empty),
        ([], [coding]) => coding
            .trim_ascii()
            .eq_ignore_ascii_case(b"chunked")
            .then_some(Framing::Chunked),
        ([first, rest @ ..], []) => {
            if rest.iter().any(|other| other != first) {
                return None;
            }
            let text = std::str::from_utf8(first).ok()?.trim();
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(Framing::Length(text.parse().ok()?))
        }
        (_, _) => None,
    }
}
