//! The text forms of proxy settings, read the same on every OS so their tests run on every CI
//! target: a proxy URL (the firm's setting and `HTTPS_PROXY`), `NO_PROXY`, and on Windows a
//! `WinHTTP` proxy string, its bypass list and a PAC result string.

use url::{Host, Url};

use super::Route;
#[cfg(any(target_os = "macos", windows, test))]
use super::answer::FailKind;
#[cfg(any(windows, test))]
use super::os::OsError;

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

/// `ERROR_WINHTTP_AUTODETECTION_FAILED`: WPAD found no PAC file URL.
#[cfg(any(windows, test))]
const AUTODETECTION_FAILED: u32 = 12_180;

/// `ERROR_WINHTTP_UNABLE_TO_DOWNLOAD_SCRIPT`.
#[cfg(any(windows, test))]
pub(super) const UNABLE_TO_DOWNLOAD_SCRIPT: u32 = 12_167;

/// `ERROR_WINHTTP_AUTOPROXY_SERVICE_ERROR`.
#[cfg(any(windows, test))]
pub(super) const AUTOPROXY_SERVICE_ERROR: u32 = 12_178;

/// `ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT`: a PAC script was got and would not run.
#[cfg(any(windows, test))]
pub(super) const BAD_AUTO_PROXY_SCRIPT: u32 = 12_166;

/// `WinHTTP`'s errors for a PAC host that does not answer: `ERROR_WINHTTP_TIMEOUT` (12002),
/// `..._NAME_NOT_RESOLVED` (12007) and `..._CANNOT_CONNECT` (12029).
#[cfg(any(windows, test))]
const PAC_HOST_SILENT: [u32; 3] = [12_002, 12_007, 12_029];

/// Where a PAC file came from, for [`pac_error`].
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy)]
pub(super) enum PacFrom {
    /// WPAD (auto-detect), by DHCP and DNS.
    AutoDetect,
    /// The PAC URL the settings name.
    Url,
}

/// What a failed PAC lookup (`WinHttpGetProxyForUrl`, its Win32 error, after the in-process
/// retry) means when nothing after it is set. From a PAC URL the firm named, any error is a
/// failed lookup, so the caller keeps an earlier answer. From auto-detect, "no proxy here" reads
/// as `DIRECT`: no WPAD file found (12180), a `wpad` name that answers but serves no script
/// (12167, a home or ISP name answering 404), and the auto-proxy service off (12178), each of
/// which would otherwise fail every lookup on that network; any other error (a bad script,
/// 12166; a timeout, 12002; and the rest) is a failed lookup. `WinHTTP` reports "no WPAD on this
/// network" and a transient discovery failure with the same code (12180), so a transient one
/// still reads as direct on a refresh.
/// A failure's kind is [`pac_kind`]'s.
#[cfg(any(windows, test))]
pub(super) fn pac_error(from: PacFrom, error: u32) -> Result<Route, OsError> {
    let none_here = [
        AUTODETECTION_FAILED,
        UNABLE_TO_DOWNLOAD_SCRIPT,
        AUTOPROXY_SERVICE_ERROR,
    ];
    let why = match from {
        PacFrom::AutoDetect if none_here.contains(&error) => return Ok(Route::Direct),
        PacFrom::AutoDetect => format!("WPAD failed (WinHTTP error {error})"),
        PacFrom::Url => format!("the PAC file failed (WinHTTP error {error})"),
    };
    Err(OsError {
        why,
        kind: pac_kind(from, error),
    })
}

/// The kind of a failed PAC lookup's Win32 error. A PAC URL that could not be fetched (12167)
/// or whose host does not answer ([`PAC_HOST_SILENT`]) is [`FailKind::PacUnreachable`]; WPAD's
/// never is, its fetch failures meaning no proxy here. A script that would not run (12166) is
/// [`FailKind::BadScript`] from either. The rest, the auto-proxy service's 12178 included, are
/// [`FailKind::Other`].
#[cfg(any(windows, test))]
pub(super) fn pac_kind(from: PacFrom, error: u32) -> FailKind {
    match (from, error) {
        (_, BAD_AUTO_PROXY_SCRIPT) => FailKind::BadScript,
        (PacFrom::Url, UNABLE_TO_DOWNLOAD_SCRIPT) => FailKind::PacUnreachable,
        (PacFrom::Url, error) if PAC_HOST_SILENT.contains(&error) => FailKind::PacUnreachable,
        _ => FailKind::Other,
    }
}

/// `kCFErrorPACFileError`: `CFNetwork`'s error for a PAC script that fails to evaluate.
#[cfg(any(target_os = "macos", test))]
const CF_PAC_FILE_ERROR: isize = 308;

/// How a macOS PAC run failed, for [`cf_pac_kind`].
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy)]
pub(super) enum CfPacFailure<'a> {
    /// `CFNetwork` called back with an error of this domain and code.
    Error(&'a str, isize),
    /// No callback within the run's 5 s.
    TimedOut,
}

/// The kind of a failed macOS PAC run: `url` for a PAC URL, else a script held inline. A
/// script that fails to evaluate (`kCFErrorPACFileError`, 308) is [`FailKind::BadScript`]. A
/// PAC URL's fetch failure (`NSURLErrorDomain`, or a negative URL-loading code in
/// `kCFErrorDomainCFNetwork`), or a PAC URL's run with no answer in 5 s, is
/// [`FailKind::PacUnreachable`]. The rest are [`FailKind::Other`].
#[cfg(any(target_os = "macos", test))]
pub(super) fn cf_pac_kind(url: bool, failure: CfPacFailure<'_>) -> FailKind {
    const CF_NETWORK: &str = "kCFErrorDomainCFNetwork";
    let not_fetched = match failure {
        CfPacFailure::Error("NSURLErrorDomain", _) | CfPacFailure::TimedOut => true,
        CfPacFailure::Error(CF_NETWORK, code) => code < 0,
        CfPacFailure::Error(..) => false,
    };
    if matches!(failure, CfPacFailure::Error(CF_NETWORK, CF_PAC_FILE_ERROR)) {
        FailKind::BadScript
    } else if url && not_fetched {
        FailKind::PacUnreachable
    } else {
        FailKind::Other
    }
}

/// What the PAC settings answer ([`pac_answer`]).
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PacAnswer {
    /// The answer, whatever else is set.
    Final(Result<Route, OsError>),
    /// WPAD's, when auto-detect is the only PAC setting and it found no route: a static proxy
    /// set beside it answers instead.
    Fallback(Result<Route, OsError>),
    /// Neither auto-detect nor a PAC URL is set.
    Unset,
}

/// The PAC settings' answer, `ask` running one lookup (its Win32 error on failure). A PAC URL
/// the settings name is asked first, so a slow WPAD (with its in-process retry) cannot use up
/// the lookup's 10 s bound before it: a firm's GPO often sets both. Its route is the answer;
/// when it fails, auto-detect's route, if it finds one, else the PAC URL's failure. With
/// auto-detect alone, its route, or its error read by [`pac_error`] as a fallback.
#[cfg(any(windows, test))]
pub(super) fn pac_answer(
    auto_detect: bool,
    url_set: bool,
    mut ask: impl FnMut(PacFrom) -> Result<Route, u32>,
) -> PacAnswer {
    if url_set {
        let failed = match ask(PacFrom::Url) {
            Ok(route) => return PacAnswer::Final(Ok(route)),
            Err(error) => pac_error(PacFrom::Url, error),
        };
        if auto_detect && let Ok(route) = ask(PacFrom::AutoDetect) {
            return PacAnswer::Final(Ok(route));
        }
        return PacAnswer::Final(failed);
    }
    if !auto_detect {
        return PacAnswer::Unset;
    }
    match ask(PacFrom::AutoDetect) {
        Ok(route) => PacAnswer::Final(Ok(route)),
        Err(error) => PacAnswer::Fallback(pac_error(PacFrom::AutoDetect, error)),
    }
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
