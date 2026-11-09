//! Tier B's network specs, checked on every OS (S15 PR7): which SIDs, ports, layers, match
//! types and flags each filter has, idempotence and extend, removal, the read-only grants, the
//! descriptors, and the ports a launch names. The launch's check against a fake WFP is in
//! `tests/check.rs`.

use std::net::Ipv4Addr;

use super::check::{ERROR_INVALID_OWNER, NetworkGap, OWNER_CHECK, WfpError, owner_allowed};
use super::spec::{
    ACTRL_ENUM, ACTRL_READ, Action, BLOCK_WEIGHT, Condition, Direction, FIREWALL_UNDER_POLICY,
    FirewallTrouble, Flags, Layer, Match, Namespace, NetworkSetup, Object, PROTOCOL_ICMP,
    PROTOCOL_ICMPV6, RuleProtocol, SpecError, allowed_users, checked_ports, denied_users,
    not_pool_sddl, permit_ports, revocations, rule_names, user_sddl,
};
use crate::ports::Slot;
use crate::test_support::TempDir;
use crate::windows::sids::Sid;
use crate::{Error, Loopback, Policy};
use clauseo_protocol::ErrorCode;

mod arbitration;
mod check;
mod ports;
mod slots;
mod writes;

pub(super) fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-11-22-33-{rid}")).unwrap()
}

pub(super) fn owner() -> Sid {
    sid(1001)
}

pub(super) fn group() -> Sid {
    sid(3000)
}

/// The slot the tests give the pool user with RID `rid`: `rid % 32` (2001 is slot 17).
pub(super) fn slot_of(rid: u32) -> Slot {
    Slot::new(u8::try_from(rid % 32).unwrap()).unwrap()
}

pub(super) fn setup(pool: &[u32]) -> NetworkSetup {
    let pool: Vec<(Slot, Sid)> = pool.iter().map(|&rid| (slot_of(rid), sid(rid))).collect();
    NetworkSetup::new(&owner(), &group(), &pool).unwrap()
}

#[test]
fn blocks_catch_the_group_and_every_pool_user_at_each_layer() {
    let ns = Namespace::production();
    let filters = setup(&[2002, 2001, 2002]).filters(&ns);
    let pool = vec![sid(2001), sid(2002)];
    let blocks = &filters[..6];
    let layers: Vec<Layer> = blocks.iter().map(|f| f.layer).collect();
    assert_eq!(
        layers,
        [
            Layer::ConnectV4,
            Layer::ConnectV6,
            Layer::RecvAcceptV4,
            Layer::RecvAcceptV6,
            Layer::AssignV4,
            Layer::AssignV6,
        ]
    );
    for (n, filter) in (0u16..).zip(blocks) {
        assert_eq!(filter.key, ns.filter_key(n), "{}", filter.name);
        assert_eq!(filter.action, Action::Block);
        assert_eq!(filter.weight, BLOCK_WEIGHT);
        let not_pool = Condition::NotPool {
            group: group(),
            pool: pool.clone(),
        };
        assert_eq!(filter.conditions[0], not_pool);
    }
    let rest: Vec<&[Condition]> = blocks.iter().map(|f| &f.conditions[1..]).collect();
    assert_eq!(
        rest,
        [
            &[][..],
            &[],
            &[Condition::NotLoopback],
            &[Condition::NotLoopback],
            &[Condition::Protocol(PROTOCOL_ICMP)],
            &[Condition::Protocol(PROTOCOL_ICMPV6)],
        ]
    );
}

#[test]
fn match_types_and_flags_are_pinned() {
    let filters = setup(&[2001]).filters(&Namespace::production());
    let block = Flags {
        persistent: true,
        clear_action_right: true,
    };
    let permit = Flags {
        persistent: true,
        clear_action_right: false,
    };
    for filter in &filters {
        let expected = if filter.action == Action::Block {
            block
        } else {
            permit
        };
        assert_eq!(filter.flags(), expected, "{}", filter.name);
    }
    let matching = |c: Condition| c.matching();
    let not_pool = Condition::NotPool {
        group: group(),
        pool: vec![sid(2001)],
    };
    assert_eq!(matching(not_pool), Match::NotEqual);
    assert_eq!(matching(Condition::Users(vec![sid(2001)])), Match::Equal);
    assert_eq!(matching(Condition::NotLoopback), Match::FlagsNoneSet);
    assert_eq!(matching(Condition::Protocol(6)), Match::Equal);
    assert_eq!(
        matching(Condition::RemoteV4(Ipv4Addr::LOCALHOST)),
        Match::Equal
    );
    assert_eq!(matching(Condition::RemotePorts(1, 2)), Match::Range);
    assert_eq!(matching(Condition::LocalPorts(1, 2)), Match::Range);
    assert_eq!(
        matching(Condition::NotUsers(vec![sid(2001)])),
        Match::NotEqual
    );
    // The address goes to WFP in host order.
    assert_eq!(u32::from(Ipv4Addr::LOCALHOST), 0x7F00_0001);
}

#[test]
fn a_rerun_over_the_same_pool_builds_the_same_objects() {
    let ns = Namespace::production();
    let (a, b) = (setup(&[2001, 2002]), setup(&[2002, 2001, 2001]));
    assert_eq!(a, b);
    assert_eq!(a.filters(&ns), b.filters(&ns));
    assert_eq!(a.grants(&ns), b.grants(&ns));
    assert_eq!(a.rules(&ns), b.rules(&ns));
}

#[test]
fn an_extend_covers_the_new_user() {
    let ns = Namespace::production();
    let after = setup(&[2001, 2002, 2003]).filters(&ns);
    for filter in &after[..12] {
        let Condition::NotPool { pool, .. } = &filter.conditions[0] else {
            panic!("{filter:?}");
        };
        assert_eq!(pool, &[sid(2001), sid(2002), sid(2003)]);
    }
    assert_eq!(after.len(), 12 + 3 * 6);
    for filter in &after[12 + 2 * 6..] {
        let named = [
            Condition::Users(vec![sid(2003)]),
            Condition::NotUsers(vec![sid(2003)]),
        ];
        assert!(named.contains(&filter.conditions[0]), "{}", filter.name);
    }
    // The users already there keep their filters, keys and all.
    let before = setup(&[2001, 2002]).filters(&ns);
    assert!(before[12..].iter().all(|f| after.contains(f)));
    let rules = setup(&[2001, 2002, 2003]).rules(&ns);
    assert!(rules.iter().all(|r| r.users.contains(sid(2003).as_str())));
}

#[test]
fn a_pool_that_would_filter_the_wrong_programs_is_refused() {
    let users = Sid::parse("S-1-5-32-545").unwrap();
    let everyone = Sid::parse("S-1-1-0").unwrap();
    let new = |group: &Sid, pool: &[Sid]| {
        let slots: Vec<(Slot, Sid)> = (0u8..)
            .zip(pool)
            .map(|(n, sid)| (Slot::new(n).unwrap(), sid.clone()))
            .collect();
        NetworkSetup::new(&owner(), group, &slots)
    };
    assert_eq!(new(&group(), &[]), Err(SpecError::NoPoolUser));
    for bad in [users, everyone] {
        let refused = Err(SpecError::NotAnAccount(bad.clone()));
        assert_eq!(new(&group(), &[sid(2001), bad.clone()]), refused);
        assert_eq!(new(&bad, &[sid(2001)]), refused, "as the group");
    }
    let owner_refused = Err(SpecError::OwnerInPool(owner()));
    assert_eq!(new(&group(), &[sid(2001), owner()]), owner_refused);
    assert_eq!(new(&owner(), &[sid(2001)]), owner_refused, "as the group");
}

#[test]
fn the_owner_gets_read_only_grants() {
    let ns = Namespace::production();
    let set_up = setup(&[2001, 2002]);
    let grants: Vec<(Object, u32)> = set_up
        .grants(&ns)
        .into_iter()
        .map(|g| (g.object, g.mask))
        .collect();
    let mut expected = vec![
        (Object::FilterContainer, ACTRL_READ | ACTRL_ENUM),
        (Object::Sublayer, ACTRL_READ),
        (Object::Provider, ACTRL_READ),
    ];
    expected.extend(
        set_up
            .filters(&ns)
            .iter()
            .map(|f| (Object::Filter(f.key), ACTRL_READ)),
    );
    assert_eq!(grants, expected);
    // FWPM_ACTRL_ADD, ADD_LINK, BEGIN_WRITE_TXN, WRITE and DELETE are never granted.
    let control = 0x1 | 0x2 | 0x8 | 0x400 | 0x0001_0000;
    assert!(grants.iter().all(|(_, mask)| mask & control == 0));
    assert_eq!(revocations(), [Object::FilterContainer]);
}

#[test]
fn the_firewall_rules_except_only_the_range() {
    let ns = Namespace::production();
    let rules = setup(&[2001, 2002]).rules(&ns);
    let users = "O:LSD:(A;;CC;;;S-1-5-21-11-22-33-2001)(A;;CC;;;S-1-5-21-11-22-33-2002)";
    let shape: Vec<(&str, Direction, RuleProtocol, Option<&str>)> = rules
        .iter()
        .map(|r| {
            assert_eq!(r.users, users);
            (
                r.name.as_str(),
                r.direction,
                r.protocol,
                r.remote_ports.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            (
                "clauseo_sandbox_block_outbound",
                Direction::Out,
                RuleProtocol::Any,
                None
            ),
            (
                "clauseo_sandbox_block_inbound",
                Direction::In,
                RuleProtocol::Any,
                None
            ),
            (
                "clauseo_sandbox_block_loopback_udp",
                Direction::Out,
                RuleProtocol::Udp,
                None
            ),
            (
                "clauseo_sandbox_block_loopback_tcp",
                Direction::Out,
                RuleProtocol::Tcp,
                Some("1-46099,46612-65023,65280-65535")
            ),
        ]
    );
}

#[test]
fn removal_covers_everything_set_up_writes() {
    for ns in [Namespace::production(), Namespace::test()] {
        let set_up = setup(&[2001, 2002, 2003]);
        let keys = ns.removal_keys();
        assert!(set_up.filters(&ns).iter().all(|f| keys.contains(&f.key)));
        let names: Vec<String> = set_up.rules(&ns).into_iter().map(|r| r.name).collect();
        assert_eq!(names, rule_names(&ns));
    }
    let (real, test) = (Namespace::production(), Namespace::test());
    assert!(
        real.removal_keys()
            .iter()
            .all(|k| !test.removal_keys().contains(k))
    );
    assert_ne!(real.provider, test.provider);
    assert_ne!(real.sublayer, test.sublayer);
    assert!(
        rule_names(&real)
            .iter()
            .all(|n| !rule_names(&test).contains(n))
    );
}

#[test]
fn production_keys_never_change() {
    let ns = Namespace::production();
    assert_eq!(ns.provider, 0xa517_0842_1321_4b98_aa1e_e79c_132f_1c10);
    assert_eq!(ns.sublayer, 0x9103_7008_b7c9_4530_b42e_2877_0023_9f22);
    assert_eq!(ns.filter_key(0), 0x674a_69a4_1c8b_4a2b_9891_6793_c5f9_0000);
    assert_eq!(ns.rules, "clauseo_sandbox");
}

#[test]
fn the_descriptors_deny_the_pool_and_allow_one_user() {
    let p = "S-1-5-21-11-22-33";
    assert_eq!(
        not_pool_sddl(&group(), &[sid(2001), sid(2002)]),
        format!(
            "O:SYG:SYD:(D;;CC;;;{p}-3000)(D;;CC;;;{p}-2001)(D;;CC;;;{p}-2002)\
             (A;;CC;;;WD)(A;;CC;;;RC)(A;;CC;;;AC)(A;;CC;;;S-1-15-2-2)"
        )
    );
    assert_eq!(
        user_sddl(&[sid(2001)]),
        format!("O:SYG:SYD:(A;;CC;;;{p}-2001)")
    );
}

#[test]
fn descriptors_read_back_only_in_set_ups_exact_shape() {
    let p = "S-1-5-21-11-22-33";
    let tail = "(A;;CC;;;WD)(A;;CC;;;RC)(A;;CC;;;AC)(A;;CC;;;S-1-15-2-2)";
    let full = "(A;;CC;;;S-1-1-0)(A;;CC;;;S-1-5-12)(A;;CC;;;S-1-15-2-1)(A;;CC;;;S-1-15-2-2)";
    let denied = Some(vec![group(), sid(2001)]);
    assert_eq!(
        denied_users(&format!("D:(D;;CC;;;{p}-3000)(D;;CC;;;{p}-2001){tail}")),
        denied
    );
    assert_eq!(
        denied_users(&format!("D:(D;;CC;;;{p}-3000)(D;;CC;;;{p}-2001){full}")),
        denied
    );
    assert_eq!(denied_users(&not_pool_sddl(&group(), &[sid(2001)])), denied);
    for odd in [
        format!("D:(D;;CC;;;{p}-3000)(A;;CC;;;WD)(A;;CC;;;RC)(A;;CC;;;AC)"),
        format!("D:(D;;CC;;;{p}-3000)(A;;CC;;;BU){tail}"),
        format!("D:(D;;CCDC;;;{p}-3000){tail}"),
        format!("D:(D;OICI;CC;;;{p}-3000){tail}"),
        format!("D:P(D;;CC;;;{p}-3000){tail}"),
        format!("D:{tail}"),
        format!("D:(D;;CC;;;WD){tail}"),
    ] {
        assert_eq!(denied_users(&odd), None, "{odd}");
    }
    assert_eq!(
        allowed_users(&format!("D:(A;;CC;;;{p}-2001)")),
        Some(vec![sid(2001)])
    );
    assert_eq!(
        allowed_users(&user_sddl(&[sid(2001), sid(2002)])),
        Some(vec![sid(2001), sid(2002)])
    );
    for odd in [
        format!("D:(A;;CC;;;{p}-2001)(D;;CC;;;{p}-2001)"),
        format!("D:(A;;CC;;;{p}-2001)(A;;CC;;;WD)"),
        format!("D:(A;;CCDC;;;{p}-2001)"),
        "D:".to_owned(),
        "O:SYG:SY".to_owned(),
    ] {
        assert_eq!(allowed_users(&odd), None, "{odd}");
    }
}

/// A policy with `ports` for its proxy and `loopback`.
fn policy(dir: &TempDir, ports: &[u16], loopback: Loopback) -> Policy {
    dir.dir("matter");
    dir.dir("state");
    let builder = Policy::builder(dir.path().join("matter"), dir.path().join("state"));
    let builder = ports.iter().fold(builder, |b, &port| b.proxy_port(port));
    builder.loopback(loopback).build().unwrap()
}

#[test]
fn a_launch_names_only_its_own_slots_ports_and_never_any() {
    let dir = TempDir::new("wfp-permit-ports");
    let listed = Loopback::Ports(vec![46150, 46191]);
    let ports = permit_ports(&policy(&dir, &[46190, 46191], listed)).unwrap();
    assert_eq!(ports, [46150, 46190, 46191]);
    let none = permit_ports(&policy(&dir, &[46190], Loopback::None)).unwrap();
    assert_eq!(none, [46190]);
    let any = permit_ports(&policy(&dir, &[46190], Loopback::Any));
    assert_eq!(any, Err(NetworkGap::LoopbackAny));
    let slot = Slot::new(2).unwrap();
    let (h, p, d) = (46_116, 46_372, 65_040);
    assert!(slot.host().contains(&h) && slot.pool().contains(&p) && slot.daemon().contains(&d));
    assert_eq!(checked_ports(slot, &[d, p, h, h]), Ok(vec![h, p, d]));
    let other = Slot::new(3).unwrap();
    for outside in [
        h - 1,
        *other.host().start(),
        *other.pool().end(),
        *other.daemon().start(),
        9222,
        61080,
    ] {
        assert_eq!(
            checked_ports(slot, &[h, outside]),
            Err(NetworkGap::PortOutsideRange(outside))
        );
    }
    assert!(
        NetworkGap::PortOutsideRange(1)
            .to_string()
            .contains("outside this matter's sandbox ports")
    );
    assert_eq!(checked_ports(slot, &[]), Ok(vec![]));
}

#[test]
fn a_stale_set_up_needs_set_up_and_the_rest_fail_the_launch() {
    let code = |cause: NetworkGap| Error::NetworkUnfiltered { cause }.code();
    for stale in [
        NetworkGap::NotSetUp,
        NetworkGap::FilterMissing("x".to_owned()),
        NetworkGap::PoolUserNotCovered,
        NetworkGap::Unreadable { call: "c", code: 5 },
        NetworkGap::ForeignFilter("x".to_owned()),
        NetworkGap::OldSetup("x".to_owned()),
    ] {
        assert_eq!(code(stale), ErrorCode::SandboxSetupRequired);
    }
    for launch in [
        NetworkGap::LoopbackAny,
        NetworkGap::PortOutsideRange(1),
        NetworkGap::EngineUnavailable {
            call: "FwpmEngineOpen0",
            code: 1753,
        },
    ] {
        assert_eq!(code(launch), ErrorCode::AgentLaunchFailed);
    }
    let foreign = NetworkGap::ForeignFilter("Hostile permit".to_owned());
    assert_eq!(
        Error::NetworkUnfiltered { cause: foreign }.to_string(),
        "a network filter set-up didn't write is in the sandbox's sublayer: Hostile permit"
    );
}

#[test]
fn each_firewall_trouble_is_named_for_firm_it() {
    for code in [0x8007_0426, 0x8007_06D9, 0x8007_06BA] {
        assert_eq!(
            FirewallTrouble::from_call("x", code),
            FirewallTrouble::ServiceStopped
        );
    }
    let com = FirewallTrouble::from_call("Rules::Add", 0x8000_4005);
    assert_eq!(
        com.words(),
        "a firewall call failed (Rules::Add, error 0x80004005), so the second layer of \
         Clauseo's network protection is off; its network filters still enforce"
    );
    let policy = FirewallTrouble::GroupPolicy("state 1".to_owned()).words();
    assert!(policy.starts_with(FIREWALL_UNDER_POLICY) && policy.contains("AllowLocalPolicyMerge"));
    assert!(
        FirewallTrouble::ServiceStopped
            .words()
            .contains("(mpssvc) isn't running")
    );
    assert!(
        FirewallTrouble::TurnedOff
            .words()
            .contains("another firewall product")
    );
    for trouble in [
        FirewallTrouble::ServiceStopped,
        FirewallTrouble::TurnedOff,
        com,
    ] {
        assert!(
            trouble.words().contains("network filters still enforce"),
            "{trouble:?}"
        );
    }
}

#[test]
fn the_base_filtering_engine_down_is_named() {
    for code in [1753, 1722, 1062, 0x8007_06D9] {
        let down = WfpError {
            call: "FwpmEngineOpen0",
            code,
        };
        assert!(
            down.cause()
                .starts_with("the Base Filtering Engine service (BFE) isn't running")
        );
    }
    let other = WfpError {
        call: "FwpmFilterAdd0",
        code: 5,
    };
    assert_eq!(other.cause(), "FwpmFilterAdd0 failed (error 0x5)");
    let owned = WfpError {
        call: OWNER_CHECK,
        code: ERROR_INVALID_OWNER,
    };
    assert!(
        owned.cause().contains("Default owner for objects"),
        "{}",
        owned.cause()
    );
}

#[test]
fn an_owner_probe_refusal_counts_only_as_access_denied_from_its_own_call() {
    let denied = |call, code| WfpError { call, code };
    assert!(denied("FwpmFilterAdd0", 5).is_access_denied_from("FwpmFilterAdd0"));
    assert!(denied("FwpmFilterAdd0", 0x8007_0005).is_access_denied_from("FwpmFilterAdd0"));
    // FWP_E_INCOMPATIBLE_CONDITION, say: refused, but not for the owner's rights.
    assert!(!denied("FwpmFilterAdd0", 0x8032_0025).is_access_denied_from("FwpmFilterAdd0"));
    assert!(!denied("FwpmEngineOpen0", 5).is_access_denied_from("FwpmFilterAdd0"));
}

#[test]
fn only_administrators_or_system_may_own_our_objects() {
    assert!(owner_allowed("S-1-5-32-544"));
    assert!(owner_allowed("S-1-5-18"));
    for other in [
        "S-1-5-21-11-22-33-1001",
        "S-1-1-0",
        "S-1-5-32-545",
        "S-1-3-4",
        "",
    ] {
        assert!(!owner_allowed(other), "{other}");
    }
}
