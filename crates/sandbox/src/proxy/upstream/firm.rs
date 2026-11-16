//! The firm's own DNS suffixes, which the egress proxy refuses on its name-only path (open mode's
//! browser listener, behind the firm's proxy, on a network whose local DNS answers no public
//! name: `proxy::name_only`). Read when that path comes on, and again each time the probe's
//! answer changes, so a VPN joined since is seen. The sources: the domain entries of `NO_PROXY`;
//! the OS's proxy exceptions (the Windows bypass list, `<local>` aside: single labels are refused
//! anyway; macOS's `ExceptionsList`); and the DNS search and VPN domains: `/etc/resolv.conf` on
//! Linux and macOS; on macOS `State:/Network/Global/DNS` `SearchDomains`, every service's
//! `SupplementalMatchDomains` and the file names in `/etc/resolver`; on Windows each adapter's
//! DNS suffix, the `Tcpip` `Domain` and `SearchList`, Group Policy's `SearchList` and primary
//! suffix, and the Name Resolution Policy Table's namespaces. If any source cannot be read, the
//! read gives none at all and the name-only path stays off; a log line names the source only.
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

/// The firm's own suffixes, normalised, from `no_proxy` (the process's `NO_PROXY`) and every
/// source this OS has, with the one log line [`learn`] writes; `None` when any source cannot
/// be read.
pub(crate) fn firm_suffixes(no_proxy: Option<&str>) -> Option<Vec<String>> {
    let no_proxy = NoProxy::parse(no_proxy.unwrap_or_default());
    let mut sources = Sources {
        no_proxy: no_proxy.domains().map(str::to_owned).collect(),
        ..Sources::default()
    };
    read_os(&mut sources);
    learn(&sources)
}

/// The suffixes of every source, and one info line on [`LIFECYCLE`] with how many usable
/// suffixes each source gave, by its fixed name, how many sources could not be read, and the
/// total. Counts only, never a name: the names are the firm's internal domains, a settings
/// value, which the log never holds (decision record 0047). `None` when a source could not be
/// read.
fn learn(sources: &Sources) -> Option<Vec<String>> {
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
    tracing::info!(
        target: LIFECYCLE,
        no_proxy = usable(&sources.no_proxy),
        resolv_conf = usable(&sources.resolv_conf),
        macos_exceptions = usable(&sources.macos_exceptions),
        macos_search_domains = usable(&sources.macos_search_domains),
        macos_supplemental = usable(&sources.macos_supplemental),
        macos_resolver_files = usable(&sources.macos_resolver_files),
        windows_bypass = usable(&sources.windows_bypass),
        adapter_dns_suffix = usable(&sources.adapter_dns_suffix),
        tcpip_registry = usable(&sources.tcpip_registry),
        windows_nrpt = usable(&sources.windows_nrpt),
        unreadable = sources.unreadable.len(),
        suffixes = found.len(),
        "egress: the firm's own names learned (counts per source; names are never logged)"
    );
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
        let text = std::fs::read_to_string("/etc/resolv.conf").ok();
        let read = text.map(|text| resolv_conf(&text));
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
mod tests {
    use std::fmt;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Level, Metadata, Subscriber};

    use super::{LIFECYCLE, Sources, bypass_entries, learn, resolv_conf, resolver_files, suffixes};
    use crate::proxy::upstream::parse::NoProxy;
    use crate::test_support::TempDir;

    /// One event: its target, level, and every field (the message included) as text.
    #[derive(Debug)]
    struct Line {
        target: String,
        level: Level,
        fields: Vec<(String, String)>,
    }

    impl Line {
        fn field(&self, name: &str) -> Option<&str> {
            let found = self.fields.iter().find(|(field, _)| field == name);
            found.map(|(_, value)| value.as_str())
        }
    }

    /// A subscriber that keeps every event.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<Line>>>);

    struct Fields(Vec<(String, String)>);

    impl Visit for Fields {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut fields = Fields(Vec::new());
            event.record(&mut fields);
            self.0.lock().unwrap().push(Line {
                target: event.metadata().target().to_owned(),
                level: *event.metadata().level(),
                fields: fields.0,
            });
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// `learn`'s suffixes from `sources`, and the events it sent.
    fn learn_captured(sources: &Sources) -> (Option<Vec<String>>, Vec<Line>) {
        let capture = Capture::default();
        let found = tracing::subscriber::with_default(capture.clone(), || learn(sources));
        let lines = std::mem::take(&mut *capture.0.lock().unwrap());
        (found, lines)
    }

    #[test]
    fn learning_logs_one_lifecycle_line_with_a_count_per_source() {
        let sources = Sources {
            no_proxy: vec!["firm.com".to_owned(), "127.0.0.1".to_owned()],
            resolv_conf: resolv_conf(
                "search corp.example canary-suffix.example
nameserver 1
",
            ),
            windows_bypass: bypass_entries("<local>;*.legal.example;10.*"),
            ..Sources::default()
        };
        let (found, lines) = learn_captured(&sources);
        let learned = [
            "canary-suffix.example",
            "corp.example",
            "firm.com",
            "legal.example",
        ];
        assert_eq!(found.unwrap(), learned);
        let [line] = lines.as_slice() else {
            panic!("one line, got {lines:?}");
        };
        assert_eq!(line.target, LIFECYCLE);
        // What the host's log file keeps at info (`log_file::kept_target`).
        assert!(line.target.starts_with("clauseo_") && line.target.ends_with("::lifecycle"));
        assert_eq!(line.level, Level::INFO);
        let counts = [
            ("no_proxy", "1"),
            ("resolv_conf", "2"),
            ("macos_exceptions", "0"),
            ("macos_search_domains", "0"),
            ("macos_supplemental", "0"),
            ("macos_resolver_files", "0"),
            ("windows_bypass", "1"),
            ("adapter_dns_suffix", "0"),
            ("tcpip_registry", "0"),
            ("windows_nrpt", "0"),
            ("unreadable", "0"),
            ("suffixes", "4"),
        ];
        for (field, count) in counts {
            assert_eq!(line.field(field), Some(count), "{field}");
        }
        assert_eq!(
            line.field("message"),
            Some(
                "egress: the firm's own names learned (counts per source; names are never logged)"
            )
        );
        assert_eq!(line.fields.len(), counts.len() + 1, "{:?}", line.fields);
    }

    #[test]
    fn the_learned_line_names_no_suffix() {
        let sources = Sources {
            resolv_conf: resolv_conf(
                "domain canary-suffix.example
search canary-two.example
",
            ),
            macos_search_domains: vec!["canary-three.example".to_owned()],
            ..Sources::default()
        };
        let (found, lines) = learn_captured(&sources);
        let found = found.unwrap();
        assert_eq!(found.len(), 3, "the canaries were learned: {found:?}");
        assert_eq!(lines.len(), 1);
        for line in &lines {
            let text = format!("{line:?}");
            assert!(!text.contains("canary"), "{text}");
            assert!(!text.contains("example"), "{text}");
        }
    }

    #[test]
    fn a_source_that_cannot_be_read_gives_no_names_at_all() {
        let sources = Sources {
            no_proxy: vec!["firm.com".to_owned()],
            unreadable: vec!["macos_supplemental"],
            ..Sources::default()
        };
        let (found, lines) = learn_captured(&sources);
        assert_eq!(found, None);
        let [line] = lines.as_slice() else {
            panic!("one line, got {lines:?}");
        };
        assert_eq!(line.field("unreadable"), Some("1"));
        assert_eq!(line.field("no_proxy"), Some("1"));
        let mut unreadable = Vec::new();
        assert_eq!(
            super::learned(&mut unreadable, "windows_nrpt", None),
            Vec::<String>::new()
        );
        assert_eq!(unreadable, ["windows_nrpt"]);
        let read = Some(vec!["firm.com".to_owned()]);
        assert_eq!(
            super::learned(&mut unreadable, "no_proxy", read),
            ["firm.com"]
        );
        assert_eq!(unreadable, ["windows_nrpt"], "a source read adds none");
    }

    #[test]
    fn a_multi_string_gives_each_namespace() {
        let mut units: Vec<u16> = ".firm.example\0vpn.firm.example\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(
            super::multi_sz(&units),
            Some(vec![
                ".firm.example".to_owned(),
                "vpn.firm.example".to_owned()
            ])
        );
        assert_eq!(super::multi_sz(&[]), Some(Vec::new()));
        units.insert(0, 0xD800);
        assert_eq!(super::multi_sz(&units), None, "not UTF-16");
    }

    #[test]
    fn resolver_files_name_their_domains() {
        let dir = TempDir::new("firm-resolver-files");
        assert_eq!(resolver_files(&dir.path().join("none")), Some(Vec::new()));
        dir.file("vpn.firm.example", "nameserver 10.1.2.3\n");
        dir.file("emea.firm.example", "nameserver 10.1.2.4\n");
        let mut names = resolver_files(dir.path()).unwrap();
        names.sort();
        assert_eq!(names, ["emea.firm.example", "vpn.firm.example"]);
    }

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
