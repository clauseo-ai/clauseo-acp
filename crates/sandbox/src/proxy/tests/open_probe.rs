//! The name-only path's probe (`name_only::Gate`): only while local DNS answers neither fixed
//! probe name does a failed lookup go to the firm's proxy by name. The probe runs at start and
//! again at most once a minute after a failed lookup; the firm's names are read when the path
//! comes on and again after the probe's answer changes, and a source that cannot be read keeps
//! the path off.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::harness::{Rig, Setup, host_denial, rig, upstream};
use super::open_upstream::OPEN;
use crate::HostRefusal::DnsUnavailable;
use crate::Upstream;
use crate::proxy::name_only::{FirmRead, PROBE_NAMES};
use crate::proxy::{Limits, ProxyMode};

/// Open mode through the upstream at `address`, with the probe answered when `dns_works`, the
/// firm's names from `read_firm` (none when `None`), and a short connect limit.
pub(super) fn probing(
    address: SocketAddr,
    dns_works: bool,
    read_firm: Option<Arc<FirmRead>>,
) -> Rig {
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    rig(Setup {
        mode: ProxyMode::Open,
        upstream: Upstream::Fixed(url),
        limits: Limits {
            connect: Duration::from_millis(200),
            ..Limits::default()
        },
        dns_works,
        read_firm,
        ..Setup::default()
    })
}

/// A read of the firm's names that gives each of `answers` in turn (the last repeats), and the
/// count of reads.
fn reads(answers: Vec<Option<Vec<String>>>) -> (Arc<FirmRead>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    let read = move || {
        let at = counted.fetch_add(1, Ordering::SeqCst);
        answers.get(at).or(answers.last()).cloned().flatten()
    };
    (Arc::new(read), count)
}

#[test]
fn where_local_dns_works_a_failed_or_timed_out_lookup_is_refused() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, true, None);
    assert_eq!(rig.browser_connect("unknown.test:443").1, 403);
    rig.net.name("slow.test", &["192.0.2.20"]);
    let _held = rig.net.hold_next_resolve();
    assert_eq!(rig.browser_connect("slow.test:443").1, 403, "timed out");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("unknown.test", 443, DnsUnavailable, false),
            host_denial("slow.test", 443, DnsUnavailable, false),
        ]
    );
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    assert_eq!(rig.net.probed(), PROBE_NAMES, "one probe, at start");
}

#[test]
fn where_local_dns_answers_no_probe_name_a_public_name_goes_by_name() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, false, None);
    assert_eq!(rig.browser_connect("www.example.com:443").1, 200);
    let head = heads.recv().unwrap();
    assert!(head.starts_with("CONNECT www.example.com:443 "), "{head}");
    assert_eq!(
        rig.net.resolved(),
        ["www.example.com"],
        "asked locally first"
    );
    assert_eq!(rig.net.probed(), PROBE_NAMES);
    assert_eq!(rig.denials(), Vec::new());
}

#[test]
fn a_source_of_the_firm_s_names_that_cannot_be_read_keeps_the_path_off() {
    let (address, heads) = upstream(OPEN);
    let (read, count) = reads(vec![None]);
    let rig = probing(address, false, Some(read));
    assert_eq!(rig.browser_connect("www.example.com:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![host_denial("www.example.com", 443, DnsUnavailable, false)]
    );
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn the_probe_runs_again_at_most_once_a_minute_after_a_failed_lookup() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, false, None);
    rig.net.name("public.test", &["192.0.2.20"]);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let probes = || rig.net.probed().len() / PROBE_NAMES.len();
    assert_eq!(code("a.test"), 200);
    assert_eq!(probes(), 1, "the one at start");
    rig.clock.advance(Duration::from_secs(59));
    assert_eq!(code("b.test"), 200);
    assert_eq!(probes(), 1, "under a minute since");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(code("public.test"), 200, "a lookup that answers");
    assert_eq!(probes(), 1, "no probe without a failed lookup");
    assert_eq!(code("c.test"), 200);
    assert_eq!(probes(), 2, "a minute since, and a failed lookup");
    assert_eq!(code("d.test"), 200);
    assert_eq!(probes(), 2);
    assert_eq!(heads.try_iter().count(), 5);
}

#[test]
fn a_change_of_the_probe_s_answer_reads_the_firm_s_names_again() {
    let (address, heads) = upstream(OPEN);
    let names = |suffix: &str| Some(vec![suffix.to_owned()]);
    let (read, count) = reads(vec![names("firm.example"), names("vpn.example")]);
    let rig = probing(address, false, Some(read));
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let minute = || rig.clock.advance(Duration::from_mins(1));
    // The path comes on at start: read once.
    assert_eq!(code("wiki.firm.example"), 403);
    assert_eq!(code("x.vpn.example"), 200);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // The probe runs again, its answer unchanged: no read.
    minute();
    assert_eq!(code("y.vpn.example"), 200);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // Local DNS answers the next probe, then no longer (a VPN joined, say).
    for probe in PROBE_NAMES {
        rig.net.answers(probe, &[&["192.0.2.80"], &[]]);
    }
    minute();
    assert_eq!(code("z.vpn.example"), 403, "the path is off");
    assert_eq!(rig.net.probed().len(), 3 * PROBE_NAMES.len());
    minute();
    assert_eq!(
        code("w.vpn.example"),
        403,
        "on again, with the names read again"
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(code("wiki.firm.example"), 200, "the old names are gone");
    assert_eq!(heads.try_iter().count(), 3);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("wiki.firm.example", 443, DnsUnavailable, false),
            host_denial("z.vpn.example", 443, DnsUnavailable, false),
            host_denial("w.vpn.example", 443, DnsUnavailable, false),
        ]
    );
}
