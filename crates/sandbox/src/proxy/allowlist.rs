//! Which hosts a launch may reach: host names as the proxy compares them, the [`Allowlist`], and
//! the starting list per login kind.

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;

use crate::Error;
use crate::proxy::shared_hosts;

/// The longest host name DNS carries, without the trailing dot.
const NAME_MAX: usize = 253;
/// The longest label.
const LABEL_MAX: usize = 63;

/// A host as the proxy compares it: one helper turns list entries and requests alike into this.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Name {
    /// ASCII (IDN through `url`'s IDNA), lowercase, no trailing dot, labels of 1 to 63 of
    /// `a-z`, `0-9`, `-` and `_`, not starting or ending with `-`.
    Domain(String),
    /// An IPv4 or IPv6 literal (IPv4 in any form the URL standard reads, `0x7f.1` included).
    Ip(IpAddr),
}

impl Name {
    /// Reads a host as a request or a list entry gives it: case is folded, one trailing dot is
    /// dropped, an IDN becomes ASCII, and `[v6]` or a dotted IPv4 becomes an address. `None` for
    /// anything else, including a bare IPv6 without brackets and an empty name.
    pub(crate) fn parse(raw: &str) -> Option<Name> {
        let raw = raw.strip_suffix('.').unwrap_or(raw);
        if raw.is_empty() {
            return None;
        }
        match url::Host::parse(raw).ok()? {
            url::Host::Domain(domain) => Name::domain(&domain),
            url::Host::Ipv4(ip) => Some(Name::Ip(IpAddr::V4(ip))),
            url::Host::Ipv6(ip) => Some(Name::Ip(IpAddr::V6(ip))),
        }
    }

    /// Checks an already ASCII domain against DNS's shape.
    fn domain(domain: &str) -> Option<Name> {
        let domain = domain.to_ascii_lowercase();
        let labels_ok = domain.split('.').all(|label| {
            (1..=LABEL_MAX).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
        (labels_ok && domain.len() <= NAME_MAX).then_some(Name::Domain(domain))
    }

    /// The host as a denial and a log line show it: the domain, or the address without brackets.
    pub(crate) fn text(&self) -> String {
        match self {
            Name::Domain(domain) => domain.clone(),
            Name::Ip(ip) => ip.to_string(),
        }
    }

    /// `host:port` for a request line or a `Host` header, with an IPv6 address in brackets.
    pub(crate) fn authority(&self, port: u16) -> String {
        match self {
            Name::Domain(domain) => format!("{domain}:{port}"),
            Name::Ip(IpAddr::V4(ip)) => format!("{ip}:{port}"),
            Name::Ip(IpAddr::V6(ip)) => format!("[{ip}]:{port}"),
        }
    }

    /// The host for a `Host` header on port 80: the port is left out.
    pub(crate) fn host_header(&self) -> String {
        match self {
            Name::Domain(domain) => domain.clone(),
            Name::Ip(IpAddr::V4(ip)) => ip.to_string(),
            Name::Ip(IpAddr::V6(ip)) => format!("[{ip}]"),
        }
    }

    /// True when the name is `domain` or under it (`a.b.example.com` is under `example.com`).
    pub(crate) fn is_at_or_under(&self, domain: &str) -> bool {
        match self {
            Name::Domain(name) => name == domain || Name::under(name, |parent| parent == domain),
            Name::Ip(_) => false,
        }
    }

    /// True when some proper parent of `name` passes `test`.
    fn under(name: &str, test: impl Fn(&str) -> bool) -> bool {
        let mut rest = name;
        while let Some((_, parent)) = rest.split_once('.') {
            if test(parent) {
                return true;
            }
            rest = parent;
        }
        false
    }

    /// True when the name is a shared-hosting domain or under one (a warning on the card, not
    /// a refusal).
    pub(crate) fn is_shared_hosting(&self) -> bool {
        shared_hosts::SHARED_HOSTS
            .iter()
            .any(|entry| self.is_at_or_under(entry))
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// The hosts one launch may reach: exact hosts and `*.domain` entries. CONNECT reaches them on
/// port 443 only and plain `http://` on port 80 only; the list holds no ports.
///
/// Matching: case-insensitive, one trailing dot ignored, an IDN compared in its ASCII form. A
/// `*.domain` entry ([`Allowlist::firm_suffix`]) matches every name under `domain`, not
/// `domain` itself. An IP literal matches only an exact entry for that address, so an address
/// is refused unless listed; a listed one still passes the proxy's address check.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Allowlist {
    exact: BTreeSet<Name>,
    suffixes: BTreeSet<String>,
}

impl fmt::Debug for Allowlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let exact = self.exact.iter().map(Name::text);
        let suffixes = self.suffixes.iter().map(|domain| format!("*.{domain}"));
        f.debug_set().entries(exact.chain(suffixes)).finish()
    }
}

impl Allowlist {
    /// A list of one host: a name (IDN allowed) or an IP literal (IPv6 in brackets).
    pub fn exact(h: &str) -> Result<Allowlist, Error> {
        let name = Name::parse(h)
            .ok_or_else(|| Error::ProxyConfig(format!("not a host name or address: {h:?}")))?;
        let mut list = Allowlist::default();
        list.exact.insert(name);
        Ok(list)
    }

    /// A list of one firm wildcard: every host under `d`. `d` may be written `example.com` or
    /// `*.example.com`. Refused: an empty, single-label or IP-literal domain, a Public Suffix
    /// List entry (private section included, so `amazonaws.com`'s `s3.amazonaws.com` and
    /// `blob.core.windows.net` are refused), and a domain on or under the shipped
    /// shared-hosting list, since a wildcard there allows strangers' sites.
    pub fn firm_suffix(d: &str) -> Result<Allowlist, Error> {
        let refuse = |why: &str| Error::ProxyConfig(format!("firm wildcard {d:?} refused: {why}"));
        let bare = d.strip_prefix("*.").unwrap_or(d);
        let domain = match Name::parse(bare) {
            Some(Name::Domain(domain)) => domain,
            Some(Name::Ip(_)) => return Err(refuse("an IP address")),
            None => return Err(refuse("not a domain")),
        };
        if !domain.contains('.') {
            return Err(refuse("a single label"));
        }
        if psl::suffix_str(&domain) == Some(domain.as_str()) {
            return Err(refuse("a public suffix"));
        }
        if Name::Domain(domain.clone()).is_shared_hosting() {
            return Err(refuse("a shared-hosting domain"));
        }
        let mut list = Allowlist::default();
        list.suffixes.insert(domain);
        Ok(list)
    }

    /// Both lists in one.
    pub fn union(mut self, other: Allowlist) -> Allowlist {
        self.exact.extend(other.exact);
        self.suffixes.extend(other.suffixes);
        self
    }

    /// True when `host` is on the list. A host that is not a valid name is not.
    pub fn contains(&self, host: &str) -> bool {
        Name::parse(host).is_some_and(|name| self.contains_name(&name))
    }

    /// True when the list holds no entry.
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.suffixes.is_empty()
    }

    pub(crate) fn contains_name(&self, name: &Name) -> bool {
        if self.exact.contains(name) {
            return true;
        }
        match name {
            Name::Domain(domain) => Name::under(domain, |parent| self.suffixes.contains(parent)),
            Name::Ip(_) => false,
        }
    }

    /// Adds one exact host (`allow`).
    pub(crate) fn insert_exact(&mut self, name: Name) {
        self.exact.insert(name);
    }

    /// Removes one exact host (`revoke`); a `*.domain` entry that covers it stays.
    pub(crate) fn remove_exact(&mut self, name: &Name) {
        self.exact.remove(name);
    }
}

/// How the agent signs in, which decides the hosts it needs before anything else is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginKind {
    /// Claude with a Claude plan (OAuth).
    ClaudePlan,
    /// Claude with an Anthropic API key.
    ClaudeApiKey,
    /// Codex signed in with a `ChatGPT` account.
    CodexChatGpt,
    /// Codex with an `OpenAI` API key.
    CodexApiKey,
    /// A firm's own endpoint (Bedrock, Vertex, or a plan's endpoint): exactly these hosts.
    Firm {
        /// The hosts, as R17 derives them from the final launch env.
        hosts: Vec<String>,
    },
}

/// The hosts every launch of this login kind may reach. Each host is inferred (research 21,
/// research 22) until G8b measures it; changes need Rohan's approval.
pub fn builtin_allowlist(kind: &LoginKind) -> Allowlist {
    let hosts: Vec<&str> = match kind {
        LoginKind::ClaudePlan => vec![
            // Inferred until G8b: the API, OAuth refresh and the paste-code sign-in page.
            "api.anthropic.com",
            "claude.ai",
            "platform.claude.com",
        ],
        // Inferred until G8b.
        LoginKind::ClaudeApiKey => vec!["api.anthropic.com"],
        LoginKind::CodexChatGpt => vec![
            // Inferred until G8b: the API, ChatGPT's backend and its token refresh.
            "api.openai.com",
            "chatgpt.com",
            "auth.openai.com",
        ],
        // Inferred until G8b.
        LoginKind::CodexApiKey => vec!["api.openai.com"],
        LoginKind::Firm { hosts } => hosts.iter().map(String::as_str).collect(),
    };
    let mut list = Allowlist::default();
    for host in hosts {
        if let Some(name) = Name::parse(host) {
            list.insert_exact(name);
        } else {
            tracing::warn!(host, "egress: firm host is not a host name; left out");
        }
    }
    list
}

#[cfg(test)]
mod tests;
