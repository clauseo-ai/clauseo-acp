//! `open_external`'s link check: which links open, and what the confirm names for each.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use clauseo_protocol::HostError;

use super::invalid;

/// The longest link `open_external` takes, in bytes, before and after the `url` crate writes it
/// back (it percent-encodes what is not ASCII, so a link can grow).
pub(crate) const MAX_LINK_BYTES: usize = 2048;

/// The longest host name, and the longest label in one, in bytes (RFC 1035).
const MAX_HOST_BYTES: usize = 253;
const MAX_LABEL_BYTES: usize = 63;

/// The longest mail address list a mailto link may show in the confirm, in bytes.
const MAX_MAIL_SHOWN: usize = 254;

/// The header fields a mailto link may carry beside its addresses, compared without case. Any
/// other (`to`, `cc`, `bcc`, `attach`, `from`, ...) would add a recipient or an attachment the
/// confirm does not name.
const MAIL_FIELDS: [&str; 2] = ["subject", "body"];

/// Where a link goes, for the confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Destination {
    /// An https or http link to this host (with its port when the link names one).
    Web(String),
    /// A mailto link to these addresses, as the link wrote them (`,` between two).
    Mail(String),
}

/// A link `open_external` may open: the URL to hand the OS, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    /// The URL as the `url` crate writes it back.
    pub(crate) url: String,
    /// Where it goes, as the confirm names it.
    pub(crate) destination: Destination,
}

/// Checks `text` is a link `open_external` may open: at most [`MAX_LINK_BYTES`] as given and as
/// written back; https or http to a public host name or address (`web_host`), or mailto to
/// plain addresses with at most a subject and a body (`mail_addresses`). Anything else is
/// `command.invalid`.
pub(crate) fn check_link(text: &str) -> Result<Link, HostError> {
    let too_long = |len: usize| {
        invalid(format!(
            "a link of {len} bytes is over the {MAX_LINK_BYTES} byte limit"
        ))
    };
    if text.len() > MAX_LINK_BYTES {
        return Err(too_long(text.len()));
    }
    let url = url::Url::parse(text).map_err(|error| invalid(format!("not a link: {error}")))?;
    if url.as_str().len() > MAX_LINK_BYTES {
        return Err(too_long(url.as_str().len()));
    }
    let destination = match url.scheme() {
        "https" | "http" => Destination::Web(web_host(&url)?),
        "mailto" => Destination::Mail(mail_addresses(&url)?),
        other => {
            return Err(invalid(format!(
                "links of the scheme {other} do not open; only https, http and mailto do"
            )));
        }
    };
    Ok(Link {
        url: url.as_str().to_owned(),
        destination,
    })
}

/// The host an https or http link goes to, as ASCII (an international name in its `xn--`
/// form, so the confirm cannot show a look-alike), with the port when the link names one.
/// Refused: a user name or password; a name over 253 bytes or with a label over 63; `localhost`
/// and names under it; and loopback, private, link-local, unspecified and broadcast addresses,
/// which the lawyer cannot judge from a confirm.
fn web_host(url: &url::Url) -> Result<String, HostError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("a link with a user name or password does not open"));
    }
    let local = || invalid("a link to this computer or its local network does not open");
    let shown = match url.host() {
        None => return Err(invalid("a web link without a host does not open")),
        Some(url::Host::Domain(name)) => {
            if name.is_empty()
                || name.len() > MAX_HOST_BYTES
                || name.split('.').any(|label| label.len() > MAX_LABEL_BYTES)
            {
                return Err(invalid("a link whose host name is too long does not open"));
            }
            let lower = name.trim_end_matches('.').to_ascii_lowercase();
            if lower == "localhost" || lower.ends_with(".localhost") {
                return Err(local());
            }
            name.to_owned()
        }
        Some(url::Host::Ipv4(address)) => {
            if !public(IpAddr::V4(address)) {
                return Err(local());
            }
            address.to_string()
        }
        Some(url::Host::Ipv6(address)) => {
            if !public(IpAddr::V6(address)) {
                return Err(local());
            }
            format!("[{address}]")
        }
    };
    Ok(match url.port() {
        Some(port) => format!("{shown}:{port}"),
        None => shown,
    })
}

/// Whether `address` is on the internet at large: not loopback, private, link-local,
/// unspecified, broadcast, or (for IPv6) unique local, and not an IPv4 address of those kinds
/// written as IPv6.
fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => public_v6(v6),
        },
    }
}

fn public_v4(v4: Ipv4Addr) -> bool {
    !(v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast())
}

fn public_v6(v6: Ipv6Addr) -> bool {
    let first = v6.segments()[0];
    let unique_local = first & 0xfe00 == 0xfc00;
    let link_local = first & 0xffc0 == 0xfe80;
    !(v6.is_loopback() || v6.is_unspecified() || unique_local || link_local)
}

/// The addresses a mailto link writes to, decoded, `,` between two. Each is `local@domain` with
/// both parts non-empty, in RFC 5322's atext and `.` (so no space, quote, angle bracket,
/// backslash, control or letter outside ASCII reaches the confirm or the mail app), at most
/// [`MAX_MAIL_SHOWN`] bytes in all. The link may carry a subject and a body ([`MAIL_FIELDS`]),
/// each once, and nothing else: no other header and no fragment.
fn mail_addresses(url: &url::Url) -> Result<String, HostError> {
    let refused = |why: &str| invalid(format!("this mailto link does not open: {why}"));
    if url.fragment().is_some() {
        return Err(refused("it has a fragment"));
    }
    let mut seen: Vec<String> = Vec::new();
    for (key, _) in url.query_pairs() {
        let key = key.to_ascii_lowercase();
        if !MAIL_FIELDS.contains(&key.as_str()) {
            return Err(refused(
                "it carries a header other than a subject or a body",
            ));
        }
        if seen.contains(&key) {
            return Err(refused("it names a subject or a body twice"));
        }
        seen.push(key);
    }
    let decoded =
        percent_decode(url.path()).ok_or_else(|| refused("its address does not decode"))?;
    if decoded.is_empty() || decoded.len() > MAX_MAIL_SHOWN {
        return Err(refused("its addresses are empty or too long"));
    }
    if !decoded.split(',').all(plain_address) {
        return Err(refused("an address is not a plain local@domain"));
    }
    Ok(decoded)
}

/// Whether `address` is `local@domain`: one `@`, both sides non-empty, every byte RFC 5322
/// atext or `.`.
fn plain_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    let atext = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&byte))
    };
    atext(local) && atext(domain)
}

/// `text` with each `%XX` decoded, when the result is UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}
