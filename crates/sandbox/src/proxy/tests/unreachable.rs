//! A PAC file that could not be fetched (decision record 0039, "A failed lookup"): the host's
//! earlier proxy while it still answers the probe, else direct; the probe faked.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::failed::{failing_rig_with, firm_proxy, set};
use super::harness::{Rig, round_trip};
use crate::Denial;
use crate::proxy::upstream::{Answer, FailKind, Failure};

/// The probes asked: host, port and cap.
type Probes = Arc<Mutex<Vec<(String, u16, Duration)>>>;

/// [`failing_rig_with`] whose probe answers `reached`, with the probes it was asked.
fn probing_rig(reached: bool) -> (Rig, super::failed::Answers, Probes) {
    let probes: Probes = Arc::default();
    let asked = Arc::clone(&probes);
    let reach = move |host: &str, port: u16, cap: Duration| {
        asked.lock().unwrap().push((host.to_owned(), port, cap));
        reached
    };
    let (rig, answers) = failing_rig_with(Some(Box::new(reach)));
    (rig, answers, probes)
}

fn unfetched() -> Answer {
    Answer::Failed(Failure::MeantProxy, FailKind::PacUnreachable)
}

/// The earlier proxy answers the probe: the tunnel goes through it, the probe capped at 1 s.
#[test]
fn an_unfetchable_pac_keeps_an_earlier_proxy_that_answers() {
    let (rig, answers, probes) = probing_rig(true);
    let (address, route, _heads) = firm_proxy();
    set(&answers, "a.test", Answer::Route(route));
    assert_eq!(rig.connect("a.test:443").1, 200);
    set(&answers, "a.test", unfetched());
    rig.clock.advance(Duration::from_mins(2));
    let (mut tunnel, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"kept"), b"kept");
    assert_eq!(rig.net.dialled(), vec![address, address]);
    let asked = probes.lock().unwrap().clone();
    let want = (
        address.ip().to_string(),
        address.port(),
        Duration::from_secs(1),
    );
    assert_eq!(asked, vec![want]);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// The earlier proxy does not answer: direct, though the firm meant a proxy, and no refusal.
#[test]
fn an_unfetchable_pac_goes_direct_when_the_earlier_proxy_is_silent() {
    let (rig, answers, probes) = probing_rig(false);
    let (address, route, _heads) = firm_proxy();
    set(&answers, "a.test", Answer::Route(route));
    assert_eq!(rig.connect("a.test:443").1, 200);
    set(&answers, "a.test", unfetched());
    rig.clock.advance(Duration::from_mins(2));
    let (mut tunnel, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"direct"), b"direct");
    let direct: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![address, direct]);
    assert_eq!(probes.lock().unwrap().len(), 1);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// No earlier proxy: direct with no probe, though the firm meant a proxy.
#[test]
fn an_unfetchable_pac_with_no_earlier_proxy_goes_direct() {
    let (rig, answers, probes) = probing_rig(true);
    set(&answers, "a.test", unfetched());
    let (mut tunnel, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"direct"), b"direct");
    let direct: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![direct]);
    assert!(probes.lock().unwrap().is_empty(), "nothing to probe");
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}
