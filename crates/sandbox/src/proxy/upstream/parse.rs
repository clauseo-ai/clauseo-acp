//! The text forms of proxy settings, read the same on every OS so their tests run on every CI
//! target: a proxy URL (the firm's setting and `HTTPS_PROXY`), `NO_PROXY`, and on Windows a
//! `WinHTTP` proxy string, its bypass list and a PAC result string.

use url::{Host, Url};

use super::Route;

/// The port of an `http://` proxy URL with none.
const HTTP_PORT: u16 = 80;

/// Why a proxy URL is not `http://host[:port]`. Never holds the URL, which may carry a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BadProxy {
    /// Not a URL.
    Unparsable,
    /// `https://`: TLS to the proxy itself, which the proxy does not speak.
    Https,
    /// Another scheme (`socks5://` and the like).
    Scheme,
    /// No host.
    NoHost,
    /// A user name or password; Clauseo stores no proxy password.
    Userinfo,
    /// A path, a query or a fragment.
    Extra,
}

impl BadProxy {
    /// The reason in words, for an error message or a log line.
    pub(super) fn reason(self) -> &'static str {
        match self {
            BadProxy::Unparsable => "it is not a URL",
            BadProxy::Https => "an https:// proxy is not supported; use http://",
            BadProxy::Scheme => "only an http:// proxy is supported",
            BadProxy::NoHost => "it has no host",
            BadProxy::Userinfo => "it has a user name or password, and Clauseo stores none",
            BadProxy::Extra => "it has a path, a query or a fragment",
        }
    }
}

/// A proxy URL read from the environment: its route, and whether a user name or password was
/// dropped from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EnvProxy {
    pub(super) route: Route,
    pub(super) dropped_userinfo: bool,
}

/// The firm's `network.upstream_proxy`, strictly: `http://host[:port]` and nothing else (a
/// trailing `/` is the same URL to the parser, so it passes).
pub(super) fn fixed_proxy(url: &Url) -> Result<Route, BadProxy> {
    check_scheme(url)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(BadProxy::Userinfo);
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(BadProxy::Extra);
    }
    http_route(url)
}

/// `HTTPS_PROXY` as curl reads it: a value with no scheme is `http://`; a user name or password
/// is dropped (and reported, never printed); a path is ignored.
pub(super) fn env_proxy(raw: &str) -> Result<EnvProxy, BadProxy> {
    let raw = raw.trim();
    let url = if raw.contains("://") {
        Url::parse(raw)
    } else {
        Url::parse(&format!("http://{raw}"))
    }
    .map_err(|_parse| BadProxy::Unparsable)?;
    check_scheme(&url)?;
    let dropped_userinfo = !url.username().is_empty() || url.password().is_some();
    Ok(EnvProxy {
        route: http_route(&url)?,
        dropped_userinfo,
    })
}

fn check_scheme(url: &Url) -> Result<(), BadProxy> {
    match url.scheme() {
        "http" => Ok(()),
        "https" => Err(BadProxy::Https),
        _ => Err(BadProxy::Scheme),
    }
}

/// The host and port of an `http://` URL; an IPv6 host comes without its brackets.
fn http_route(url: &Url) -> Result<Route, BadProxy> {
    let host = match url.host() {
        Some(Host::Domain(name)) if !name.is_empty() => name.to_owned(),
        Some(Host::Ipv4(ip)) => ip.to_string(),
        Some(Host::Ipv6(ip)) => ip.to_string(),
        Some(Host::Domain(_)) | None => return Err(BadProxy::NoHost),
    };
    Ok(Route::Http {
        host,
        port: url.port().unwrap_or(HTTP_PORT),
    })
}

/// A host as matching compares it: lower case, no trailing dot, no IPv6 brackets.
pub(super) fn normal_host(host: &str) -> String {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Splits `host:port`, `[v6]:port`, `[v6]` or a bare host. A bare IPv6 address (two or more
/// colons, no brackets) has no port. `None` when a port is there but is not a number.
fn split_host_port(entry: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = entry.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port.parse().ok()?),
            None if after.is_empty() => None,
            None => return None,
        };
        return Some((normal_host(host), port));
    }
    match entry.split_once(':') {
        Some((host, port)) if !port.contains(':') => {
            Some((normal_host(host), Some(port.parse().ok()?)))
        }
        _ => Some((normal_host(entry), None)),
    }
}

/// `NO_PROXY`: a comma list; `*` alone means every host; an entry matches the host and its
/// subdomains (a leading `.` or `*.` changes nothing); an entry with a port matches only that
/// port. Entries that do not parse are skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NoProxy {
    all: bool,
    entries: Vec<(String, Option<u16>)>,
}

impl NoProxy {
    pub(super) fn parse(raw: &str) -> NoProxy {
        let mut list = NoProxy::default();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" {
                list.all = true;
                continue;
            }
            let entry = entry.strip_prefix("*.").unwrap_or(entry);
            let entry = entry.strip_prefix('.').unwrap_or(entry);
            if let Some((host, port)) = split_host_port(entry)
                && !host.is_empty()
            {
                list.entries.push((host, port));
            }
        }
        list
    }

    /// Whether `host` (already [`normal_host`]) on `port` goes direct.
    pub(super) fn matches(&self, host: &str, port: u16) -> bool {
        self.all
            || self.entries.iter().any(|(entry, entry_port)| {
                entry_port.is_none_or(|entry_port| entry_port == port) && is_within(host, entry)
            })
    }
}

/// `host` is `domain` or one of its subdomains.
fn is_within(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain)
        .is_some_and(|head| head.is_empty() || head.ends_with('.'))
}

/// A `WinHTTP` proxy string, as `WinHttpGetIEProxyConfigForCurrentUser` and
/// `WinHttpGetProxyForUrl` give it: entries split by `;` or white space, each `host[:port]` or
/// `scheme=host[:port]`, a host perhaps written `http://host`. For a CONNECT the first `https=`
/// entry wins, then the first entry with no scheme; `http=`, `ftp=` and `socks=` entries never
/// carry HTTPS. `None`: no entry serves HTTPS, so the connection goes direct.
#[cfg(any(windows, test))]
pub(super) fn winhttp_proxy(raw: &str) -> Option<Route> {
    let entries = || {
        raw.split(|c: char| c == ';' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .map(|entry| match entry.split_once('=') {
                Some((scheme, value)) => (Some(scheme.trim().to_ascii_lowercase()), value.trim()),
                None => (None, entry),
            })
    };
    let https = entries().find_map(|(scheme, value)| {
        if scheme.as_deref() == Some("https") {
            winhttp_server(value)
        } else {
            None
        }
    });
    https.or_else(|| {
        entries().find_map(|(scheme, value)| scheme.is_none().then(|| winhttp_server(value))?)
    })
}

/// One server of a `WinHTTP` proxy string. An `https://` server (TLS to the proxy) is skipped.
#[cfg(any(windows, test))]
fn winhttp_server(value: &str) -> Option<Route> {
    if value.contains("://") {
        return Url::parse(value)
            .ok()
            .and_then(|url| env_proxy(url.as_str()).ok())
            .map(|proxy| proxy.route);
    }
    let (host, port) = split_host_port(value)?;
    (!host.is_empty()).then(|| Route::Http {
        host,
        port: port.unwrap_or(HTTP_PORT),
    })
}

/// A PAC result string (`PROXY a:1; SOCKS5 b:2; DIRECT`): the first `PROXY` or `HTTP` entry,
/// or `DIRECT`, in order. `SOCKS*` entries and `HTTPS` (TLS to the proxy) are skipped; with no
/// usable entry the connection goes direct.
#[cfg(any(windows, test))]
pub(super) fn pac_result(raw: &str) -> Route {
    for entry in raw
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let mut words = entry.split_whitespace();
        let keyword = words.next().unwrap_or_default().to_ascii_uppercase();
        match keyword.as_str() {
            "DIRECT" => return Route::Direct,
            "PROXY" | "HTTP" => {
                if let Some(route) = words.next().and_then(winhttp_server) {
                    return route;
                }
            }
            _ => {}
        }
    }
    Route::Direct
}

/// Whether a proxy string is in PAC form (`PROXY a:1; DIRECT`) rather than `WinHTTP`'s.
#[cfg(any(windows, test))]
pub(super) fn is_pac_form(raw: &str) -> bool {
    let first = raw.split_whitespace().next().unwrap_or_default();
    [
        "PROXY", "DIRECT", "HTTP", "HTTPS", "SOCKS", "SOCKS4", "SOCKS5",
    ]
    .iter()
    .any(|keyword| first.trim_end_matches(';').eq_ignore_ascii_case(keyword))
}

/// The bypass list of the Windows static proxy setting: entries split by `;`, `,` or white
/// space; `<local>` matches a host with no dot; other entries are patterns where `*` matches any
/// run of characters, compared without case against the host, or against `host:port` when the
/// pattern names a port. A leading `scheme://` on an entry is ignored.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Bypass {
    local: bool,
    patterns: Vec<String>,
}

#[cfg(any(windows, test))]
impl Bypass {
    pub(super) fn parse(raw: &str) -> Bypass {
        let mut list = Bypass::default();
        for entry in raw
            .split(|c: char| c == ';' || c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
        {
            if entry.eq_ignore_ascii_case("<local>") {
                list.local = true;
                continue;
            }
            let entry = entry.split_once("://").map_or(entry, |(_, rest)| rest);
            list.patterns.push(entry.to_ascii_lowercase());
        }
        list
    }

    /// Whether `host` (already [`normal_host`]) on `port` goes direct.
    pub(super) fn matches(&self, host: &str, port: u16) -> bool {
        if self.local && !host.contains('.') && !host.contains(':') {
            return true;
        }
        let with_port = format!("{host}:{port}");
        self.patterns.iter().any(|pattern| {
            let names_port = pattern.rsplit_once(':').is_some_and(|(_, tail)| {
                !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'*')
            }) && !host.contains(':');
            glob(pattern, if names_port { &with_port } else { host })
        })
    }
}

/// `*`-only wildcard match over bytes, iterative with one backtrack point.
#[cfg(any(windows, test))]
fn glob(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match (pattern.get(p), text.get(t)) {
            (Some(b'*'), _) => {
                star = Some((p, t));
                p += 1;
            }
            (Some(&want), Some(&have)) if want == have => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((star_p, star_t)) => {
                    p = star_p + 1;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|&b| b == b'*'))
}
