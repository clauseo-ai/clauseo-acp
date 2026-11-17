//! The firm's own suffixes: each source's parse, the read's fail-closed rule and its log line.

use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

use super::{
    Counts, LIFECYCLE, Sources, bypass_entries, learn, resolv_conf, resolv_conf_file,
    resolver_files, store_absent, suffixes,
};
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

/// `learn`'s suffixes from `sources` after a read whose counts were `last`, and the
/// events it sent.
fn learn_after(sources: &Sources, last: &mut Option<Counts>) -> (Option<Vec<String>>, Vec<Line>) {
    let capture = Capture::default();
    let found = tracing::subscriber::with_default(capture.clone(), || learn(sources, last));
    let lines = std::mem::take(&mut *capture.0.lock().unwrap());
    (found, lines)
}

/// `learn`'s suffixes from `sources` on a first read, and the events it sent.
fn learn_captured(sources: &Sources) -> (Option<Vec<String>>, Vec<Line>) {
    let capture = Capture::default();
    let found = tracing::subscriber::with_default(capture.clone(), || learn(sources, &mut None));
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
        Some("egress: the firm's own names learned (counts per source; names are never logged)")
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
fn the_learned_line_is_written_again_only_when_a_count_changes() {
    let mut sources = Sources {
        no_proxy: vec!["firm.com".to_owned()],
        ..Sources::default()
    };
    let mut last = None;
    let (_, lines) = learn_after(&sources, &mut last);
    assert_eq!(lines.len(), 1, "the first read");
    for _probe in 0..3 {
        let (found, lines) = learn_after(&sources, &mut last);
        assert_eq!(found.unwrap(), ["firm.com"]);
        assert_eq!(lines.len(), 0, "nothing changed: {lines:?}");
    }
    sources.resolv_conf = vec!["vpn.example".to_owned()];
    let (_, lines) = learn_after(&sources, &mut last);
    let [line] = lines.as_slice() else {
        panic!("one line, got {lines:?}");
    };
    assert_eq!(line.field("resolv_conf"), Some("1"));
    assert_eq!(line.field("suffixes"), Some("2"));
}

#[test]
fn a_missing_resolv_conf_gives_none_and_an_unreadable_one_turns_the_path_off() {
    let dir = TempDir::new("firm-resolv-conf");
    let missing = resolv_conf_file(&dir.path().join("resolv.conf"));
    assert_eq!(missing, Some(Vec::new()), "not there: empty");
    let sources = Sources {
        no_proxy: vec!["firm.com".to_owned()],
        resolv_conf: missing.unwrap(),
        ..Sources::default()
    };
    assert_eq!(
        learn_captured(&sources).0.unwrap(),
        ["firm.com"],
        "the path can come on"
    );
    dir.file("resolv.conf", "search corp.example\n");
    let found = resolv_conf_file(&dir.path().join("resolv.conf"));
    assert_eq!(found, Some(vec!["corp.example".to_owned()]));
    // A read error: a directory where the file is.
    assert_eq!(resolv_conf_file(&dir.dir("dir")), None);
    let mut unreadable = Vec::new();
    let read = super::learned(&mut unreadable, "resolv_conf", None);
    assert_eq!(read, Vec::<String>::new());
    assert_eq!(unreadable, ["resolv_conf"], "unreadable keeps the path off");
}

#[test]
fn a_dynamic_store_key_not_there_gives_none_and_any_other_failure_is_unreadable() {
    assert_eq!(store_absent(1004), Some(Vec::new()), "kSCStatusNoKey");
    for status in [0, 1001, 1003, 2001] {
        assert_eq!(store_absent(status), None, "{status}");
    }
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
