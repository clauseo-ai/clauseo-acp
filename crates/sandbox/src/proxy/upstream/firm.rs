//! The firm's own DNS suffixes, which the egress proxy refuses on its name-only path (open mode's
//! browser listener, behind the firm's proxy, when local DNS could not check a name:
//! `proxy::name_only`). Read once, when the proxy starts, from the domain entries of `NO_PROXY`,
//! the OS's proxy exceptions (the Windows bypass list, `<local>` aside: single labels are refused
//! anyway; macOS's `ExceptionsList`) and the DNS search domains (`/etc/resolv.conf` on Linux and
//! macOS; macOS's `State:/Network/Global/DNS` `SearchDomains`; on Windows each adapter's DNS
//! suffix, and the `Tcpip` `Domain` and `SearchList`, Group Policy's included). A source that
//! cannot be read adds nothing, with a log line naming the source only: the fixed list of
//! internal suffixes still holds. An entry that is an IP address, a network or a known public
//! suffix (`com`, `co.uk`) is dropped: it is no firm's own, and would refuse half the web.

#[cfg(any(windows, test))]
use super::parse::Bypass;
use super::parse::NoProxy;
use crate::proxy::allowlist::Name;

/// The firm's own suffixes, normalised, from `no_proxy` (the process's `NO_PROXY`) and every
/// source this OS has.
pub(crate) fn firm_suffixes(no_proxy: Option<&str>) -> Vec<String> {
    let no_proxy = NoProxy::parse(no_proxy.unwrap_or_default());
    let mut entries: Vec<String> = no_proxy.domains().map(str::to_owned).collect();
    entries.extend(os_entries());
    suffixes(entries.iter().map(String::as_str))
}

/// Logs a source that could not be read, by name only, and adds nothing for it.
pub(super) fn unread(source: &'static str) {
    tracing::warn!(
        source,
        "egress: a source of the firm's own names could not be read; the fixed list still holds"
    );
}

/// `read`'s entries, or none (logged as [`unread`]) when it gives `None`.
fn learned(source: &'static str, read: Option<Vec<String>>) -> Vec<String> {
    read.unwrap_or_else(|| {
        unread(source);
        Vec::new()
    })
}

/// The OS's proxy exceptions and DNS search domains, raw.
fn os_entries() -> Vec<String> {
    let mut entries = Vec::new();
    #[cfg(unix)]
    entries.extend(learned(
        "resolv.conf",
        std::fs::read_to_string("/etc/resolv.conf")
            .ok()
            .map(|text| resolv_conf(&text)),
    ));
    #[cfg(target_os = "macos")]
    {
        entries.extend(learned("macos_exceptions", super::macos::exceptions()));
        entries.extend(learned(
            "macos_search_domains",
            super::macos::search_domains(),
        ));
    }
    #[cfg(windows)]
    {
        let bypass = super::windows::bypass_list().map(|raw| bypass_entries(&raw));
        entries.extend(learned("windows_bypass", bypass));
        let adapters = crate::proxy::local_net::dns_suffixes().ok();
        entries.extend(learned("adapter_dns_suffix", adapters));
        entries.extend(super::windows::dns_search_list());
    }
    entries
}

/// The domains of a `resolv.conf`'s `search` and `domain` lines, both kept (the resolver uses
/// the last, but either names the firm's own). A line starting `#` or `;` is a comment.
#[cfg(any(unix, test))]
pub(super) fn resolv_conf(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.split(['#', ';']).next().unwrap_or_default();
        let mut words = line.split_whitespace();
        if matches!(words.next(), Some("search" | "domain")) {
            found.extend(words.map(str::to_owned));
        }
    }
    found
}

/// The patterns of a Windows bypass list (`<local>` is dropped: single labels are refused on
/// the name-only path anyway).
#[cfg(any(windows, test))]
pub(super) fn bypass_entries(raw: &str) -> Vec<String> {
    Bypass::parse(raw).patterns().to_vec()
}

/// The suffixes of raw entries, normalised by [`suffix`], sorted, each once.
fn suffixes<'a>(entries: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut found: Vec<String> = entries.into_iter().filter_map(suffix).collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// One entry as a suffix: a port dropped (`*.firm.com:8080`), then the text after the last `*`
/// (`*.firm.com` and `intra*.firm.com` give `firm.com`) with its leading dots dropped, read as
/// a host name ([`Name::parse`]: lower case, ASCII). `None` for an IP address, a network
/// (`169.254/16`), a pattern with nothing after its last `*` (`10.*`), and a known public
/// suffix.
fn suffix(entry: &str) -> Option<String> {
    let entry = entry.trim();
    let entry = match entry.rsplit_once(':') {
        Some((host, port))
            if !host.contains(':') && port.bytes().all(|b| b.is_ascii_digit() || b == b'*') =>
        {
            host
        }
        _ => entry,
    };
    let entry = entry.rsplit_once('*').map_or(entry, |(_, rest)| rest);
    let entry = entry.trim_start_matches('.');
    if entry.contains('/') {
        return None;
    }
    let Some(Name::Domain(domain)) = Name::parse(entry) else {
        return None;
    };
    let public = psl::suffix(domain.as_bytes())
        .is_some_and(|known| known.is_known() && known.as_bytes() == domain.as_bytes());
    (!public).then_some(domain)
}

#[cfg(test)]
mod tests {
    use super::{bypass_entries, resolv_conf, suffixes};
    use crate::proxy::upstream::parse::NoProxy;

    fn learned(entries: &[String]) -> Vec<String> {
        suffixes(entries.iter().map(String::as_str))
    }

    #[test]
    fn resolv_conf_gives_its_search_and_domain_lines() {
        let text = "# made by NetworkManager\nnameserver 10.0.0.53\n; old\n\
                    domain Firm.Example.\nsearch corp.firm.example  emea.firm.example # x\n\
                    options ndots:2\nsearchx no.example\n";
        assert_eq!(
            learned(&resolv_conf(text)),
            ["corp.firm.example", "emea.firm.example", "firm.example"]
        );
        assert_eq!(resolv_conf("nameserver 1.1.1.1\n"), Vec::<String>::new());
    }

    #[test]
    fn the_windows_bypass_list_gives_its_domains() {
        let raw = "<local>;*.firm.com; 10.*;192.168.1.1 intra*.emea.firm.net,\
                   http://*.legal.example:8080;[fe80::1];*.com;*";
        assert_eq!(
            learned(&bypass_entries(raw)),
            ["emea.firm.net", "firm.com", "legal.example"]
        );
    }

    #[test]
    fn macos_exceptions_give_their_domains() {
        let raw = [
            "*.local",
            "169.254/16",
            "firm.example",
            "*.Firm.Example",
            "10.0.0.0/8",
        ];
        let entries: Vec<String> = raw.iter().map(|entry| (*entry).to_owned()).collect();
        assert_eq!(learned(&entries), ["firm.example", "local"]);
    }

    #[test]
    fn search_domains_drop_public_suffixes_and_addresses() {
        let raw = [
            "firm.example",
            "co.uk",
            "com",
            "github.io",
            "10.1.2.3",
            "firm",
            "::1",
        ];
        let entries: Vec<String> = raw.iter().map(|entry| (*entry).to_owned()).collect();
        assert_eq!(learned(&entries), ["firm", "firm.example"]);
    }

    #[test]
    fn no_proxy_gives_its_domain_entries() {
        let raw = "localhost,127.0.0.1,.firm.com,*.corp.example,10.0.0.0/8,fileserver:8080,*";
        let no_proxy = NoProxy::parse(raw);
        let entries: Vec<String> = no_proxy.domains().map(str::to_owned).collect();
        assert_eq!(
            learned(&entries),
            ["corp.example", "fileserver", "firm.com", "localhost"]
        );
    }
}
