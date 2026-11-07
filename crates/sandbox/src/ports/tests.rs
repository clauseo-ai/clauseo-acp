//! The port registry and binds: claims never collide, closed ports stay retired on macOS and
//! Linux, browser claims stay out of [`TIER_B_PORTS`], and Windows binds stay in it.

use super::*;

#[test]
fn binds_loopback() {
    let listener = bind_loopback(ListenerFor::TierB).unwrap();
    let address = listener.local_addr().unwrap();
    assert!(address.ip().is_loopback(), "{address}");
    assert_ne!(address.port(), 0);
    #[cfg(windows)]
    assert!(TIER_B_PORTS.contains(&address.port()), "{address}");
}

#[test]
fn a_full_range_names_itself_and_the_windows_reservations() {
    let last = io::Error::from(io::ErrorKind::PermissionDenied);
    let error = range_exhausted(&last);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let text = error.to_string();
    assert!(text.contains("46100..=46199"), "{text}");
    assert!(text.contains("another program"), "{text}");
    assert!(
        text.contains("netsh int ipv4 show excludedportrange protocol=tcp"),
        "{text}"
    );
}

#[test]
fn two_claims_differ_and_each_is_loopback_bindable() {
    let (a, b) = (claim_port().unwrap(), claim_port().unwrap());
    assert_ne!(a.port(), b.port());
    assert_ne!(a.port(), 0);
    #[cfg(windows)]
    assert!(TIER_B_PORTS.contains(&a.port()), "{a:?}");
    assert!(format!("{a:?}").contains(&a.port().to_string()), "{a:?}");
}

#[test]
fn a_claimed_port_is_never_handed_out_again() {
    let claim = claim_port().unwrap();
    for _ in 0..32 {
        let listener = bind_loopback(ListenerFor::TierB).unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), claim.port());
        assert_ne!(claim_port().unwrap().port(), claim.port());
    }
}

#[cfg(not(windows))]
#[test]
fn a_bind_that_lands_on_a_claimed_port_is_asked_again() {
    let claim = claim_port().unwrap();
    // The claim binds nothing, so the test can put a listener on its port.
    let mut listeners = vec![imp::bind_port(claim.port()).unwrap()].into_iter();
    let got = imp::first_unheld(&Held::lock(), || {
        listeners.next().map_or_else(|| imp::bind_port(0), Ok)
    })
    .unwrap();
    assert_ne!(got.local_addr().unwrap().port(), claim.port());
    let all_claimed = imp::first_unheld(&Held::lock(), || imp::bind_port(claim.port()));
    assert!(all_claimed.is_err(), "the port was bound twice");
}

#[cfg(not(windows))]
#[test]
fn a_closed_listener_port_is_not_handed_out_again() {
    let listener = bind_loopback(ListenerFor::TierB).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let error = claim_port_at(port).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
    for _ in 0..200 {
        assert_ne!(claim_port().unwrap().port(), port);
    }
}

#[cfg(not(windows))]
#[test]
fn a_released_claim_is_not_handed_out_again() {
    let claim = claim_port().unwrap();
    let port = claim.port();
    drop(claim);
    let error = claim_port_at(port).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
    for _ in 0..200 {
        let next = claim_port().unwrap();
        assert_ne!(next.port(), port);
    }
    // The OS may hand the port back to a bind of port 0; the retired port is asked again.
    for _ in 0..200 {
        let listener = bind_loopback(ListenerFor::TierB).unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), port);
    }
}

#[test]
fn the_candidate_loop_never_binds_a_claimed_port() {
    let claim = claim_port().unwrap();
    // The lock is held across the loop: the hook binds a socket and never touches the
    // registry.
    let held = Held::lock();
    // A free port other than the claim's, so the loop's only skip is the claimed one.
    let other = loop {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        if port != claim.port() && !held.contains(port) {
            break port;
        }
    };
    let mut tried = Vec::new();
    let got = first_free([claim.port(), other].into_iter(), &held, |port| {
        tried.push(port);
        TcpListener::bind("127.0.0.1:0")
    })
    .unwrap();
    assert_eq!(tried, vec![other]);
    drop(got);
    let mut tried = Vec::new();
    let none = first_free([claim.port()].into_iter(), &held, |port| {
        tried.push(port);
        TcpListener::bind("127.0.0.1:0")
    });
    assert!(none.is_err());
    assert_eq!(tried, Vec::<u16>::new());
}

#[test]
fn claim_at_checks_the_registry_before_its_test_bind() {
    let claim = claim_port().unwrap();
    let mut binds = 0;
    let error = claim_at_with(claim.port(), |_| {
        binds += 1;
        TcpListener::bind("127.0.0.1:0")
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
    assert_eq!(binds, 0);
}

#[test]
fn claim_at_refuses_a_claimed_port_and_a_listening_one() {
    let claim = claim_port().unwrap();
    let error = claim_port_at(claim.port()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
    let listener = bind_loopback(ListenerFor::TierB).unwrap();
    let port = listener.local_addr().unwrap().port();
    let error = claim_port_at(port).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
    let error = claim_port_at(0).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
}

#[cfg(windows)]
#[test]
fn dropping_a_claim_frees_its_port() {
    let claim = claim_port().unwrap();
    let port = claim.port();
    assert!(claim_port_at(port).is_err());
    drop(claim);
    let again = claim_port_at(port).unwrap();
    assert_eq!(again.port(), port);
}

#[cfg(windows)]
#[test]
fn claim_at_refuses_a_port_outside_the_tier_b_range() {
    for port in [46_099, 46_200, 61_080, 80] {
        let error = claim_port_at(port).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{port}: {error}");
    }
    let start = claim_port_at(*TIER_B_PORTS.start());
    let end = claim_port_at(*TIER_B_PORTS.end());
    for claim in [start, end] {
        match claim {
            Ok(claim) => assert!(TIER_B_PORTS.contains(&claim.port())),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}"),
        }
    }
}

#[cfg(windows)]
#[test]
fn a_second_bind_with_reuseaddr_fails() {
    use socket2::{Domain, Protocol, Socket, Type};
    let listener = bind_loopback(ListenerFor::TierB).unwrap();
    let address = listener.local_addr().unwrap();
    let other = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    other.set_reuse_address(true).unwrap();
    assert!(
        other.bind(&address.into()).is_err(),
        "{address} bound twice"
    );
}

#[test]
fn the_tier_b_range_is_below_the_dynamic_range_and_clear_of_known_listeners() {
    assert_eq!(TIER_B_PORTS.len(), 100);
    assert!(
        *TIER_B_PORTS.end() < 49_152,
        "Windows' default dynamic range starts at 49152"
    );
    // WinRM, the ASP.NET state service, EtherNet/IP, SQL Server's browser, BACnet.
    for known in [47_001, 42_424, 44_818, 1_434, 47_808] {
        assert!(!TIER_B_PORTS.contains(&known), "{known}");
    }
}

#[test]
fn a_browser_claim_never_lands_in_the_tier_b_range() {
    for _ in 0..64 {
        let claim = claim_browser_port().unwrap();
        assert!(!TIER_B_PORTS.contains(&claim.port()), "{claim:?}");
    }
    // A bind that lands in the range, or on a claimed port, is asked again.
    let claim = claim_port().unwrap();
    let held = Held::lock();
    // A listener already bound on a free port in the range, so another process (nextest runs
    // the crate's tests in several) can't take that port between choosing and binding it.
    let mut in_range = Some(
        TIER_B_PORTS
            .clone()
            .filter(|port| !held.contains(*port))
            .find_map(|port| imp::bind_port(port).ok())
            .unwrap(),
    );
    // The claim binds nothing, so the test can put a listener on its port.
    let mut claimed = Some(claim.port());
    let mut asked = 0;
    let got = outside_tier_b(&held, || {
        asked += 1;
        match in_range.take() {
            Some(listener) => Ok(listener),
            None => imp::bind_port(claimed.take().unwrap_or(0)),
        }
    })
    .unwrap();
    assert_eq!(asked, 3);
    assert!(!TIER_B_PORTS.contains(&got.local_addr().unwrap().port()));
}

#[test]
fn a_browser_claim_at_refuses_tier_b_ports_and_zero() {
    for port in [0, *TIER_B_PORTS.start(), *TIER_B_PORTS.end(), 46_150] {
        let error = claim_browser_port_at(port).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{port}: {error}");
    }
    let free = claim_browser_port().unwrap();
    let port = free.port();
    drop(free);
    match claim_browser_port_at(port) {
        Ok(claim) => assert_eq!(claim.port(), port),
        // macOS and Linux retire a dropped claim's port.
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}"),
    }
}

#[test]
fn every_listener_binds_127_0_0_1_only() {
    let listener = bind_loopback(ListenerFor::TierB).unwrap();
    assert_eq!(
        listener.local_addr().unwrap().ip(),
        std::net::IpAddr::from(std::net::Ipv4Addr::LOCALHOST)
    );
}

#[test]
fn a_listener_for_anything_but_tier_b_never_lands_in_the_range() {
    for _ in 0..64 {
        let listener = bind_loopback(ListenerFor::Other).unwrap();
        let address = listener.local_addr().unwrap();
        assert!(!TIER_B_PORTS.contains(&address.port()), "{address}");
        assert_eq!(
            address.ip(),
            std::net::IpAddr::from(std::net::Ipv4Addr::LOCALHOST)
        );
    }
}
