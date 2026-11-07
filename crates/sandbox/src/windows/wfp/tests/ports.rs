//! Tier B's port check against a machine in memory (S15 PR7): a dynamic range or an excluded
//! port overlapping `TIER_B_PORTS` refuses, and so does a listener there that isn't this app's.

use super::super::check::NetworkGap;
use super::super::port_check::{Listener, PortsSeen, dynamic_span, is_ours, ports_usable};

/// Windows' default dynamic range, 49152 to 65535.
fn default_range() -> PortsSeen {
    PortsSeen {
        dynamic: vec![dynamic_span(49_152, 16_384).unwrap()],
        ..PortsSeen::default()
    }
}

/// A listener on `port` of process `pid` running `image`.
fn listener(port: u16, pid: u32, image: Option<&str>) -> Listener {
    Listener {
        port,
        pid,
        image: image.map(str::to_owned),
    }
}

/// This app: process 100, running Clauseo.exe.
fn ours(listener: &Listener) -> bool {
    is_ours(listener, 100, Some(r"C:\Program Files\Clauseo\Clauseo.exe"))
}

#[test]
fn the_default_dynamic_range_and_its_old_one_leave_the_range_alone() {
    assert_eq!(dynamic_span(49_152, 16_384), Some((49_152, 65_535)));
    assert_eq!(dynamic_span(1_025, 3_976), Some((1_025, 5_000)));
    assert_eq!(dynamic_span(60_000, 0), None);
    assert_eq!(ports_usable(&default_range(), ours), Ok(()));
    let old = PortsSeen {
        dynamic: vec![(1_025, 5_000), (49_152, 65_535)],
        ..PortsSeen::default()
    };
    assert_eq!(ports_usable(&old, ours), Ok(()));
}

#[test]
fn an_overlapping_dynamic_range_refuses() {
    for (start, count) in [
        (1_024, 64_511),
        (46_199, 10),
        (40_000, 6_101),
        (10_000, 50_000),
    ] {
        let seen = PortsSeen {
            dynamic: vec![(49_152, 65_535), dynamic_span(start, count).unwrap()],
            ..PortsSeen::default()
        };
        let refused = ports_usable(&seen, ours);
        assert!(
            matches!(refused, Err(NetworkGap::DynamicPortsOverlap { first, .. }) if first == start),
            "{start}+{count}: {refused:?}"
        );
        assert!(!refused.unwrap_err().needs_setup());
    }
    let next_to_it = PortsSeen {
        dynamic: vec![dynamic_span(46_200, 3_000).unwrap(), (40_000, 46_099)],
        ..PortsSeen::default()
    };
    assert_eq!(ports_usable(&next_to_it, ours), Ok(()));
}

#[test]
fn an_excluded_port_in_the_range_refuses() {
    let seen = PortsSeen {
        reserved: vec![50_000, 46_150],
        ..default_range()
    };
    let refused = ports_usable(&seen, ours).unwrap_err();
    assert_eq!(refused, NetworkGap::PortsReserved(46_150));
    assert!(
        refused.to_string().contains("excludedportrange"),
        "{refused}"
    );
    let outside = PortsSeen {
        reserved: vec![50_000, 46_099],
        ..default_range()
    };
    assert_eq!(ports_usable(&outside, ours), Ok(()));
}

#[test]
fn a_listener_in_the_range_must_be_this_app_s() {
    let app = r"c:\program files\clauseo\CLAUSEO.EXE";
    let fine = PortsSeen {
        listeners: vec![
            listener(46_100, 100, None),
            listener(46_101, 200, Some(app)),
            listener(9_222, 300, Some(r"C:\Chrome\chrome.exe")),
        ],
        ..default_range()
    };
    assert_eq!(ports_usable(&fine, ours), Ok(()));
    let chrome = PortsSeen {
        listeners: vec![listener(46_120, 300, Some(r"C:\Chrome\chrome.exe"))],
        ..default_range()
    };
    assert_eq!(
        ports_usable(&chrome, ours),
        Err(NetworkGap::ForeignListener {
            port: 46_120,
            program: r"C:\Chrome\chrome.exe".to_owned()
        })
    );
    // http.sys, as System, whose program can't be read.
    let system = PortsSeen {
        listeners: vec![listener(46_199, 4, None)],
        ..default_range()
    };
    assert_eq!(
        ports_usable(&system, ours),
        Err(NetworkGap::ForeignListener {
            port: 46_199,
            program: "process 4".to_owned()
        })
    );
}
