//! The name-only path's probe (`name_only::Gate`): only while local DNS answers no fixed probe
//! name does a failed lookup go to the firm's proxy by name. The probe runs at start and again
//! after a failed lookup: at most once a minute while the path is off, after ten seconds while
//! it is on, and at once when a lookup has answered since. The firm's names are read on every
//! probe that finds the path on, and a source that cannot be read keeps the path off.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use super::harness::{Rig, Setup, host_denial, rig, upstream};
use super::open_upstream::OPEN;
use super::test_net::TestNet;
use crate::HostRefusal::DnsUnavailable;
use crate::Upstream;
use crate::proxy::name_only::{FirmRead, probe_names};
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
    assert_eq!(rig.net.probed(), probe_names(), "one probe, at start");
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
    assert_eq!(rig.net.probed(), probe_names());
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
fn while_the_path_is_off_the_probe_runs_again_at_most_once_a_minute() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, true, None);
    rig.net.name("public.test", &["192.0.2.20"]);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let probes = || rig.net.probed().len() / probe_names().len();
    assert_eq!(code("a.test"), 403);
    assert_eq!(probes(), 1, "the one at start");
    assert_eq!(code("public.test"), 200, "a lookup that answers");
    rig.clock.advance(Duration::from_secs(59));
    assert_eq!(code("b.test"), 403);
    assert_eq!(probes(), 1, "under a minute since, the path off");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(code("public.test"), 200);
    assert_eq!(probes(), 1, "no probe without a failed lookup");
    assert_eq!(code("c.test"), 403);
    assert_eq!(probes(), 2, "a minute since, and a failed lookup");
    assert_eq!(code("d.test"), 403);
    assert_eq!(probes(), 2);
    assert_eq!(heads.try_iter().count(), 2);
}

#[test]
fn while_the_path_is_on_the_probe_runs_again_after_ten_seconds() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, false, None);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let probes = || rig.net.probed().len() / probe_names().len();
    assert_eq!(code("a.test"), 200);
    assert_eq!(probes(), 1, "the one at start");
    rig.clock.advance(Duration::from_secs(9));
    assert_eq!(code("b.test"), 200);
    assert_eq!(probes(), 1, "under ten seconds since");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(code("c.test"), 200);
    assert_eq!(probes(), 2, "ten seconds since, and a failed lookup");
    assert_eq!(heads.try_iter().count(), 3);
}

#[test]
fn once_a_lookup_answers_while_the_path_is_on_the_next_failure_probes_at_once() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, false, None);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let probes = || rig.net.probed().len() / probe_names().len();
    assert_eq!(code("a.test"), 200, "the path is on");
    // Local DNS comes back: the probe names answer from now on.
    for probe in probe_names() {
        rig.net.name(probe, &["192.0.2.80"]);
    }
    rig.net.name("public.test", &["192.0.2.20"]);
    assert_eq!(
        code("b.test"),
        200,
        "no lookup has answered yet: the last probe holds"
    );
    assert_eq!(probes(), 1);
    assert_eq!(code("public.test"), 200, "a lookup that answers");
    assert_eq!(probes(), 1, "no probe without a failed lookup");
    assert_eq!(code("c.test"), 403, "probed again at once: the path is off");
    assert_eq!(probes(), 2);
    assert_eq!(
        rig.denials(),
        vec![host_denial("c.test", 443, DnsUnavailable, false)]
    );
    assert_eq!(
        heads.try_iter().count(),
        3,
        "a.test, b.test and public.test"
    );
}

#[test]
fn every_probe_while_the_path_is_on_reads_the_firm_s_names_again() {
    let (address, heads) = upstream(OPEN);
    let names = |suffix: &str| Some(vec![suffix.to_owned()]);
    let (read, count) = reads(vec![names("firm.example"), names("vpn.example")]);
    let rig = probing(address, false, Some(read));
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    assert_eq!(code("wiki.firm.example"), 403);
    assert_eq!(code("x.vpn.example"), 200);
    assert_eq!(count.load(Ordering::SeqCst), 1, "read at start");
    // A VPN joined while the network stays name-only: the next probe reads the new suffix.
    rig.clock.advance(Duration::from_secs(10));
    assert_eq!(code("y.vpn.example"), 403, "the new suffix is refused");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(code("wiki.firm.example"), 200, "the old one is gone");
    assert_eq!(heads.try_iter().count(), 2);
}

#[test]
fn a_change_of_the_probe_s_answer_turns_the_path_off_and_on_again() {
    let (address, heads) = upstream(OPEN);
    let names = |suffix: &str| Some(vec![suffix.to_owned()]);
    let (read, count) = reads(vec![names("firm.example"), names("vpn.example")]);
    let rig = probing(address, false, Some(read));
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    let minute = || rig.clock.advance(Duration::from_mins(1));
    assert_eq!(code("x.vpn.example"), 200);
    // Local DNS answers the next probe, then no longer.
    for probe in probe_names() {
        rig.net.answers(probe, &[&["192.0.2.80"], &[]]);
    }
    minute();
    assert_eq!(code("z.vpn.example"), 403, "the path is off");
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "no read while local DNS works"
    );
    minute();
    assert_eq!(
        code("w.vpn.example"),
        403,
        "on again, with the names read again"
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(code("wiki.firm.example"), 200);
    assert_eq!(heads.try_iter().count(), 2);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("z.vpn.example", 443, DnsUnavailable, false),
            host_denial("w.vpn.example", 443, DnsUnavailable, false),
        ]
    );
}

#[test]
fn the_first_request_waits_for_the_first_probe_not_the_connect_limit() {
    let (address, heads) = upstream(OPEN);
    let net = Arc::new(TestNet::default());
    let probe = net.hold_next_probe();
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    // A connect limit past nextest's 30 s: waiting it out fails the test by timing out.
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        upstream: Upstream::Fixed(url),
        limits: Limits {
            connect: Duration::from_secs(60),
            ..Limits::default()
        },
        net,
        ..Setup::default()
    });
    probe.arrived.recv().unwrap();
    // The page's lookup never answers; the probe ends (no answer) once the lookup is asked.
    let lookup = rig.net.hold_next_resolve();
    let arrived = lookup.arrived;
    let release = probe.release;
    let releaser = thread::spawn(move || {
        arrived.recv().unwrap();
        release.send(()).unwrap();
    });
    assert_eq!(rig.browser_connect("slow.test:443").1, 200, "by name");
    releaser.join().unwrap();
    assert!(heads.recv().unwrap().starts_with("CONNECT slow.test:443 "));
    drop(lookup.release);
}
