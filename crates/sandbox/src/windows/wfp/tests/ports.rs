//! Tier B's port check against a machine in memory (S15 PR7): a dynamic range overlapping
//! `TIER_B_PORTS` refuses every launch; a slot's block wholly reserved or taken refuses that
//! slot's launches only; a foreign listener on one host port is skipped, not refused.

use super::super::check::NetworkGap;
use super::super::port_check::{
    LaunchPorts, Listener, PortsSeen, dynamic_span, is_ours, ports_usable,
};
use crate::ports::Slot;

fn slot(index: u8) -> Slot {
    Slot::new(index).unwrap()
}

/// A launch on `slot` naming no ports yet (`check_slot`).
fn launch(index: u8) -> LaunchPorts<'static> {
    LaunchPorts {
        slot: slot(index),
        ports: &[],
    }
}

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

/// This app: process 100.
fn ours(listener: &Listener) -> bool {
    is_ours(listener, 100)
}

#[test]
fn the_default_dynamic_range_and_its_old_one_leave_the_range_alone() {
    assert_eq!(dynamic_span(49_152, 16_384), Some((49_152, 65_535)));
    assert_eq!(dynamic_span(1_025, 3_976), Some((1_025, 5_000)));
    assert_eq!(dynamic_span(60_000, 0), None);
    assert_eq!(
        ports_usable(&default_range(), Some(launch(0)), ours),
        Ok(())
    );
    let old = PortsSeen {
        dynamic: vec![(1_025, 5_000), (49_152, 65_535)],
        ..PortsSeen::default()
    };
    assert_eq!(ports_usable(&old, Some(launch(0)), ours), Ok(()));
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
        let refused = ports_usable(&seen, Some(launch(0)), ours);
        assert!(
            matches!(refused, Err(NetworkGap::DynamicPortsOverlap { first, .. }) if first == start),
            "{start}+{count}: {refused:?}"
        );
        assert!(!refused.unwrap_err().needs_setup());
    }
    let next_to_it = PortsSeen {
        dynamic: vec![dynamic_span(46_612, 3_000).unwrap(), (40_000, 46_099)],
        ..PortsSeen::default()
    };
    assert_eq!(ports_usable(&next_to_it, Some(launch(0)), ours), Ok(()));
}

#[test]
fn set_up_checks_the_dynamic_range_only() {
    let seen = PortsSeen {
        reserved: vec![46_150],
        listeners: vec![listener(46_100, 300, None)],
        ..default_range()
    };
    assert_eq!(ports_usable(&seen, None, ours), Ok(()));
    let overlapping = PortsSeen {
        dynamic: vec![(40_000, 50_000)],
        ..PortsSeen::default()
    };
    assert!(ports_usable(&overlapping, None, ours).is_err());
}

#[test]
fn a_foreign_listener_on_one_host_port_leaves_its_slot_launching_and_the_claim_skips_it() {
    // Inedo's agent on 46336, slot 29's fourth host port.
    let inedo = r"C:\Program Files\Inedo Agent\InedoAgentService.exe";
    let seen = PortsSeen {
        listeners: vec![listener(46_336, 300, Some(inedo))],
        ..default_range()
    };
    for index in [0, 28, 29, 30, 31] {
        assert_eq!(
            ports_usable(&seen, Some(launch(index)), ours),
            Ok(()),
            "slot {index}"
        );
    }
    // Slot 29's host listeners then bind on the 7 others (the claim's skip, `first_free`).
    let s = slot(29);
    assert_eq!(s.host().filter(|port| *port != 46_336).count(), 7);
    assert!(s.host().contains(&46_336));
    // Its proxy bound on 46_332: the policy names it, and that one is ours.
    let mut ours_too = seen;
    ours_too.listeners.push(listener(46_332, 100, None));
    let named = Some(LaunchPorts {
        slot: s,
        ports: &[46_332, 46_333],
    });
    assert_eq!(ports_usable(&ours_too, named, ours), Ok(()));
}

#[test]
fn a_foreign_listener_on_a_port_the_launch_names_refuses_it() {
    let chrome = r"C:\Chrome\chrome.exe";
    let seen = PortsSeen {
        listeners: vec![
            listener(46_100, 300, Some(chrome)),
            listener(46_101, 4, None),
        ],
        ..default_range()
    };
    let named = |ports: &'static [u16]| {
        Some(LaunchPorts {
            slot: slot(0),
            ports,
        })
    };
    assert_eq!(
        ports_usable(&seen, named(&[46_100]), ours),
        Err(NetworkGap::ForeignListener {
            port: 46_100,
            program: chrome.to_owned()
        })
    );
    assert_eq!(
        ports_usable(&seen, named(&[46_102, 46_101]), ours),
        Err(NetworkGap::ForeignListener {
            port: 46_101,
            program: "process 4".to_owned()
        })
    );
    // Another process running Clauseo's own program is foreign too (pid, not program file).
    let app = r"C:\Program Files\Clauseo\Clauseo.exe";
    let other = PortsSeen {
        listeners: vec![
            listener(46_102, 200, Some(app)),
            listener(46_103, 100, Some(app)),
        ],
        ..default_range()
    };
    assert!(ports_usable(&other, named(&[46_102]), ours).is_err());
    assert_eq!(ports_usable(&other, named(&[46_103]), ours), Ok(()));
}

#[test]
fn an_excluded_range_covering_a_slots_block_refuses_that_slot_only() {
    // Hyper-V reserved 46350-46449: slot 31's host block (46348-46355) only in part, slot 0's
    // to slot 11's browser blocks (46356-46451) in part or whole.
    let reserved: Vec<u16> = (46_350..=46_449).collect();
    let seen = PortsSeen {
        reserved,
        ..default_range()
    };
    for index in 0..=11 {
        let refused = ports_usable(&seen, Some(launch(index)), ours);
        let whole = *slot(index).pool().end() <= 46_449;
        if whole {
            let gap = refused.unwrap_err();
            assert_eq!(
                gap,
                NetworkGap::SlotPortsUnusable {
                    slot: slot(index),
                    block: "browser",
                    first: *slot(index).pool().start(),
                    last: *slot(index).pool().end(),
                }
            );
            assert!(!gap.needs_setup());
            let text = gap.to_string();
            assert!(text.contains("excludedportrange"), "{text}");
            let account = format!("ClauseoSb{:02}", index + 1);
            assert!(text.contains(&format!("sandbox user {account})")), "{text}");
            assert!(!text.contains("slot"), "{text}");
        } else {
            assert_eq!(refused, Ok(()), "slot {index}");
        }
    }
    // Slot 31's host block is only partly reserved: its launches go on.
    assert_eq!(ports_usable(&seen, Some(launch(31)), ours), Ok(()));
    for index in 12..31 {
        assert_eq!(
            ports_usable(&seen, Some(launch(index)), ours),
            Ok(()),
            "slot {index}"
        );
    }
}

#[test]
fn a_host_block_reserved_or_held_in_whole_refuses_that_slot() {
    // Slot 2's host ports, 46116-46123: five reserved, three held by another program.
    let s = slot(2);
    let seen = PortsSeen {
        reserved: (46_116..=46_120).collect(),
        listeners: (46_121..=46_123)
            .map(|port| listener(port, 300, None))
            .collect(),
        ..default_range()
    };
    assert!(matches!(
        ports_usable(&seen, Some(launch(2)), ours),
        Err(NetworkGap::SlotPortsUnusable {
            block: "host",
            first: 46_116,
            last: 46_123,
            ..
        })
    ));
    assert_eq!(*s.host().start(), 46_116);
    assert_eq!(ports_usable(&seen, Some(launch(1)), ours), Ok(()));
    assert_eq!(ports_usable(&seen, Some(launch(3)), ours), Ok(()));
    // One of them its own (this process's) listener: one port is still usable.
    let mut one_ours = seen;
    one_ours.listeners[0].pid = 100;
    assert_eq!(ports_usable(&one_ours, Some(launch(2)), ours), Ok(()));
    // Block D, 8 ports reserved: slot 2's browser daemon block.
    let daemon = PortsSeen {
        reserved: s.daemon().collect(),
        ..default_range()
    };
    assert!(matches!(
        ports_usable(&daemon, Some(launch(2)), ours),
        Err(NetworkGap::SlotPortsUnusable {
            block: "browser daemon",
            ..
        })
    ));
}
