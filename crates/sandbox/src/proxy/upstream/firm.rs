//! The firm's own DNS suffixes, which the egress proxy refuses on its name-only path (open mode's
//! browser listener, behind the firm's proxy, on a network whose local DNS answers no public
//! name: `proxy::name_only`). Read again on every probe that finds local DNS unable to answer,
//! so a VPN joined since is seen. The sources: the domain entries of `NO_PROXY`;
//! the OS's proxy exceptions (the Windows bypass list, `<local>` aside: single labels are refused
//! anyway; macOS's `ExceptionsList`); and the DNS search and VPN domains: `/etc/resolv.conf` on
//! Linux and macOS; on macOS `State:/Network/Global/DNS` `SearchDomains`, every service's
//! `SupplementalMatchDomains` and the file names in `/etc/resolver`; on Windows each adapter's
//! DNS suffix, the `Tcpip` `Domain` and `SearchList`, Group Policy's `SearchList` and primary
//! suffix, and the Name Resolution Policy Table's namespaces. A source that is not there (no
//! `/etc/resolv.conf`, no `/etc/resolver`, no DNS state in the dynamic store) gives none; if any
//! source is there but cannot be read, the read gives none at all and the name-only path stays
//! off; a log line names the source only.
//! An entry that is an IP address, a network or a known public suffix (`com`, `co.uk`) is
//! dropped: it is no firm's own, and would refuse half the web.

#[cfg(any(windows, test))]
use super::parse::Bypass;
use super::parse::NoProxy;
use crate::proxy::allowlist::Name;
use crate::proxy::name_only::LIFECYCLE;

/// Each source's raw entries, by the source's fixed name (the log line's field names), and the
/// names of those that could not be read.
#[derive(Debug, Default)]
struct Sources {
    no_proxy: Vec<String>,
    resolv_conf: Vec<String>,
    macos_exceptions: Vec<String>,
    macos_search_domains: Vec<String>,
    macos_supplemental: Vec<String>,
    macos_resolver_files: Vec<String>,
    windows_bypass: Vec<String>,
    adapter_dns_suffix: Vec<String>,
    tcpip_registry: Vec<String>,
    windows_nrpt: Vec<String>,
    unreadable: Vec<&'static str>,
}

/// The counts of [`learn`]'s line, in its field order: kept between reads so the line is
/// written only when they change.
pub(crate) type Counts = [usize; 12];

/// The firm's own suffixes, normalised, from `no_proxy` (the process's `NO_PROXY`) and every
/// source this OS has, with the log line [`learn`] writes when the counts differ from `last`;
/// `None` when any source cannot be read.
pub(crate) fn firm_suffixes(
    no_proxy: Option<&str>,
    last: &mut Option<Counts>,
) -> Option<Vec<String>> {
    let no_proxy = NoProxy::parse(no_proxy.unwrap_or_default());
    let mut sources = Sources {
        no_proxy: no_proxy.domains().map(str::to_owned).collect(),
        ..Sources::default()
    };
    read_os(&mut sources);
    learn(&sources, last)
}

/// The suffixes of every source, and, when the counts differ from `last` (kept), one info
/// line on [`LIFECYCLE`] with how many usable suffixes each source gave, by its fixed name, how
/// many sources could not be read, and the total. Counts only, never a name: the names are the
/// firm's internal domains, a settings value, which the log never holds (decision record 0047).
/// `None` when a source could not be read.
fn learn(sources: &Sources, last: &mut Option<Counts>) -> Option<Vec<String>> {
    let usable = |entries: &[String]| {
        entries
            .iter()
            .map(String::as_str)
            .filter_map(suffix)
            .count()
    };
    let all = [
        &sources.no_proxy,
        &sources.resolv_conf,
        &sources.macos_exceptions,
        &sources.macos_search_domains,
        &sources.macos_supplemental,
        &sources.macos_resolver_files,
        &sources.windows_bypass,
        &sources.adapter_dns_suffix,
        &sources.tcpip_registry,
        &sources.windows_nrpt,
    ];
    let found = suffixes(
        all.iter()
            .flat_map(|entries| entries.iter().map(String::as_str)),
    );
    let counts: Counts = [
        usable(&sources.no_proxy),
        usable(&sources.resolv_conf),
        usable(&sources.macos_exceptions),
        usable(&sources.macos_search_domains),
        usable(&sources.macos_supplemental),
        usable(&sources.macos_resolver_files),
        usable(&sources.windows_bypass),
        usable(&sources.adapter_dns_suffix),
        usable(&sources.tcpip_registry),
        usable(&sources.windows_nrpt),
        sources.unreadable.len(),
        found.len(),
    ];
    if last.replace(counts) != Some(counts) {
        let [
            no_proxy,
            resolv_conf,
            macos_exceptions,
            macos_search_domains,
            macos_supplemental,
            macos_resolver_files,
            windows_bypass,
            adapter_dns_suffix,
            tcpip_registry,
            windows_nrpt,
            unreadable,
            suffixes,
        ] = counts;
        tracing::info!(
            target: LIFECYCLE,
            no_proxy,
            resolv_conf,
            macos_exceptions,
            macos_search_domains,
            macos_supplemental,
            macos_resolver_files,
            windows_bypass,
            adapter_dns_suffix,
            tcpip_registry,
            windows_nrpt,
            unreadable,
            suffixes,
            "egress: the firm's own names learned (counts per source; names are never logged)"
        );
    }
    sources.unreadable.is_empty().then_some(found)
}

/// `read`'s entries; none when it gives `None`, with `source` logged by name and kept in
/// `unreadable`.
fn learned(
    unreadable: &mut Vec<&'static str>,
    source: &'static str,
    read: Option<Vec<String>>,
) -> Vec<String> {
    read.unwrap_or_else(|| {
        tracing::warn!(
            source,
            "egress: a source of the firm's own names could not be read; the name-only path is off"
        );
        unreadable.push(source);
        Vec::new()
    })
}

/// The OS's proxy exceptions and DNS search and VPN domains, raw, each into its source.
fn read_os(sources: &mut Sources) {
    let unreadable = &mut sources.unreadable;
    #[cfg(unix)]
    {
        let read = resolv_conf_file(std::path::Path::new("/etc/resolv.conf"));
        sources.resolv_conf = learned(unreadable, "resolv_conf", read);
    }
    #[cfg(target_os = "macos")]
    {
        let read = super::macos::exceptions();
        sources.macos_exceptions = learned(unreadable, "macos_exceptions", read);
        let read = super::macos::search_domains();
        sources.macos_search_domains = learned(unreadable, "macos_search_domains", read);
        let read = super::macos::supplemental_domains();
        sources.macos_supplemental = learned(unreadable, "macos_supplemental", read);
        let read = resolver_files(std::path::Path::new("/etc/resolver"));
        sources.macos_resolver_files = learned(unreadable, "macos_resolver_files", read);
    }
    #[cfg(windows)]
    {
        let read = super::windows::bypass_list().map(|raw| bypass_entries(&raw));
        sources.windows_bypass = learned(unreadable, "windows_bypass", read);
        let read = crate::proxy::local_net::dns_suffixes().ok();
        sources.adapter_dns_suffix = learned(unreadable, "adapter_dns_suffix", read);
        let read = super::windows::dns_search_list();
        sources.tcpip_registry = learned(unreadable, "tcpip_registry", read);
        let read = super::windows::nrpt_namespaces();
        sources.windows_nrpt = learned(unreadable, "windows_nrpt", read);
    }
    #[cfg(not(any(unix, windows)))]
    let _none = unreadable;
}

/// The file names in `dir` (`/etc/resolver`, where a VPN client puts one file per domain it
/// resolves); none when `dir` does not exist, `None` when it or a name cannot be read.
#[cfg(any(target_os = "macos", test))]
fn resolver_files(dir: &std::path::Path) -> Option<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_unreadable) => return None,
    };
    entries
        .map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect()
}

/// The domains of the `resolv.conf` at `path` ([`resolv_conf`]); none when there is no such
/// file (a machine with no DNS server set), `None` when it cannot be read.
#[cfg(any(unix, test))]
fn resolv_conf_file(path: &std::path::Path) -> Option<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(resolv_conf(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(_unreadable) => None,
    }
}

/// What a dynamic store read that gave nothing means, by `SCError()`'s status: none when the
/// key is not there (`kSCStatusNoKey`, 1004: no DNS set, or a service gone), `None` for any
/// other status (the store could not be read).
#[cfg(any(target_os = "macos", test))]
pub(super) fn store_absent(status: i32) -> Option<Vec<String>> {
    const NO_KEY: i32 = 1004;
    (status == NO_KEY).then(Vec::new)
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

/// The strings of a registry `REG_MULTI_SZ` (or a `REG_SZ`'s one), empty ones skipped; `None`
/// when one is not UTF-16.
#[cfg(any(windows, test))]
pub(super) fn multi_sz(units: &[u16]) -> Option<Vec<String>> {
    units
        .split(|unit| *unit == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf16(part).ok())
        .collect()
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
mod tests;
