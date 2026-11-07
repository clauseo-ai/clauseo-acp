//! Tier B's network specs, checked on every OS (S15 PR7): which SIDs, ports and layers each
//! filter names, idempotence and extend, removal, the permit from a policy, and the launch's
//! fail-closed check against a fake WFP.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use super::check::{FoundFilter, NetworkGap, SetupReader, check_setup};
use super::spec::{
    ACTRL_ADD, ACTRL_ADD_LINK, ACTRL_READ, Action, BLOCK_WEIGHT, Condition, Direction, Layer,
    Namespace, NetworkSetup, Object, PERMIT_LAYER, PERMIT_WEIGHT, PROTOCOL_ICMP, PROTOCOL_ICMPV6,
    PROTOCOL_TCP, RuleProtocol, SpecError, block_filters, checked_ports, grants, permit_filters,
    permit_ports, revocations, rule_names, sddl_users, user_sddl,
};
use crate::test_support::TempDir;
use crate::windows::sids::Sid;
use crate::{Error, Loopback, Policy};
use clauseo_protocol::ErrorCode;

fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-11-22-33-{rid}")).unwrap()
}

fn owner() -> Sid {
    sid(1001)
}

fn setup(pool: &[u32]) -> NetworkSetup {
    let pool: Vec<Sid> = pool.iter().map(|&rid| sid(rid)).collect();
    NetworkSetup::new(&owner(), &pool).unwrap()
}

#[test]
fn block_filters_name_every_pool_user_at_each_layer() {
    let ns = Namespace::production();
    let filters = setup(&[2002, 2001, 2002]).filters(&ns);
    let pool = vec![sid(2001), sid(2002)];
    let layers: Vec<Layer> = filters.iter().map(|f| f.layer).collect();
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
    for (n, filter) in (0u16..).zip(&filters) {
        assert_eq!(filter.key, Some(ns.filter_key(n)), "{}", filter.name);
        assert_eq!(filter.action, Action::Block);
        assert_eq!(filter.weight, BLOCK_WEIGHT);
        assert_eq!(filter.conditions[0], Condition::Users(pool.clone()));
    }
    let rest: Vec<&[Condition]> = filters.iter().map(|f| &f.conditions[1..]).collect();
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
fn no_block_filter_excepts_a_port_or_an_address() {
    // The range is excepted only in the firewall rules: a block that left 61080-61179 open
    // would let matter B's agent reach matter A's ports (spec.rs).
    for filter in setup(&[2001]).filters(&Namespace::production()) {
        assert!(
            filter
                .conditions
                .iter()
                .all(|c| !matches!(c, Condition::RemotePort(_) | Condition::RemoteV4(_))),
            "{filter:?}"
        );
    }
}

#[test]
fn a_rerun_over_the_same_pool_builds_the_same_objects() {
    let ns = Namespace::production();
    let (a, b) = (setup(&[2001, 2002]), setup(&[2002, 2001, 2001]));
    assert_eq!(a, b);
    assert_eq!(a.filters(&ns), b.filters(&ns));
    assert_eq!(a.rules(&ns), b.rules(&ns));
}

#[test]
fn an_extend_covers_the_new_user_under_the_same_keys() {
    let ns = Namespace::production();
    let before = setup(&[2001, 2002]).filters(&ns);
    let after = setup(&[2001, 2002, 2003]).filters(&ns);
    for (old, new) in before.iter().zip(&after) {
        assert_eq!(old.key, new.key);
        assert_eq!(
            new.conditions[0],
            Condition::Users(vec![sid(2001), sid(2002), sid(2003)])
        );
    }
    let rules = setup(&[2001, 2002, 2003]).rules(&ns);
    assert!(rules.iter().all(|r| r.users.contains(sid(2003).as_str())));
}

#[test]
fn a_pool_that_would_filter_the_wrong_programs_is_refused() {
    let users = Sid::parse("S-1-5-32-545").unwrap();
    let everyone = Sid::parse("S-1-1-0").unwrap();
    assert_eq!(NetworkSetup::new(&owner(), &[]), Err(SpecError::NoPoolUser));
    for group in [users, everyone] {
        assert_eq!(
            NetworkSetup::new(&owner(), &[sid(2001), group.clone()]),
            Err(SpecError::NotAnAccount(group))
        );
    }
    assert_eq!(
        NetworkSetup::new(&owner(), &[sid(2001), owner()]),
        Err(SpecError::OwnerInPool(owner()))
    );
}

#[test]
fn the_owner_gets_pr0s_three_grants_and_reads() {
    let ns = Namespace::production();
    let grants: Vec<(Object, u32)> = grants(&ns)
        .into_iter()
        .map(|g| (g.object, g.mask))
        .collect();
    let mut expected = vec![
        (Object::Sublayer, ACTRL_ADD_LINK | ACTRL_READ),
        (Object::FilterContainer, ACTRL_ADD | ACTRL_READ),
        (Object::Layer(Layer::ConnectV4), ACTRL_ADD_LINK | ACTRL_READ),
        (Object::Provider, ACTRL_READ),
    ];
    expected.extend((0..6).map(|n| (Object::Filter(ns.filter_key(n)), ACTRL_READ)));
    assert_eq!(grants, expected);
    assert_eq!(PERMIT_LAYER, Layer::ConnectV4);
}

#[test]
fn the_firewall_rules_except_only_the_range() {
    let ns = Namespace::production();
    let rules = setup(&[2001, 2002]).rules(&ns);
    let users = "O:LSD:(A;;CC;;;S-1-5-21-11-22-33-2001)(A;;CC;;;S-1-5-21-11-22-33-2002)";
    let shape: Vec<(&str, Direction, RuleProtocol, &str, Option<&str>)> = rules
        .iter()
        .map(|r| {
            assert_eq!(r.users, users);
            let ports = r.remote_ports.as_deref();
            (
                r.name.as_str(),
                r.direction,
                r.protocol,
                r.remote_addresses,
                ports,
            )
        })
        .collect();
    let non_loopback = super::spec::NON_LOOPBACK_ADDRESSES;
    let loopback = super::spec::LOOPBACK_ADDRESSES;
    assert_eq!(
        shape,
        [
            (
                "clauseo_sandbox_block_outbound",
                Direction::Out,
                RuleProtocol::Any,
                non_loopback,
                None
            ),
            (
                "clauseo_sandbox_block_inbound",
                Direction::In,
                RuleProtocol::Any,
                non_loopback,
                None
            ),
            (
                "clauseo_sandbox_block_loopback_udp",
                Direction::Out,
                RuleProtocol::Udp,
                loopback,
                None
            ),
            (
                "clauseo_sandbox_block_loopback_tcp",
                Direction::Out,
                RuleProtocol::Tcp,
                loopback,
                Some("1-61079,61180-65535")
            ),
        ]
    );
}

#[test]
fn removal_covers_everything_set_up_writes() {
    for ns in [Namespace::production(), Namespace::test()] {
        let set_up = setup(&[2001]);
        let keys = ns.removal_keys();
        assert!(
            set_up
                .filters(&ns)
                .iter()
                .all(|f| keys.contains(&f.key.unwrap()))
        );
        let names: Vec<String> = set_up.rules(&ns).into_iter().map(|r| r.name).collect();
        assert_eq!(names, rule_names(&ns));
        let granted: Vec<Object> = grants(&ns).iter().map(|g| g.object).collect();
        assert!(revocations().iter().all(|o| granted.contains(o)));
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
fn the_user_descriptor_grants_match_to_each_sid() {
    assert_eq!(
        user_sddl(&[sid(2001), sid(2002)]),
        "O:SYG:SYD:(A;;CC;;;S-1-5-21-11-22-33-2001)(A;;CC;;;S-1-5-21-11-22-33-2002)"
    );
}

#[test]
fn only_plain_match_allows_read_as_coverage() {
    let a = "S-1-5-21-11-22-33-2001";
    let b = "S-1-5-21-11-22-33-2002";
    assert_eq!(
        sddl_users(&format!("D:(A;;CC;;;{a})(A;;CC;;;{b})")),
        Some(vec![sid(2001), sid(2002)])
    );
    assert_eq!(sddl_users(&user_sddl(&[sid(2001)])), Some(vec![sid(2001)]));
    for odd in [
        format!("D:(A;;CC;;;{a})(D;;CC;;;{a})"),
        format!("D:(A;;CC;;;{a})(A;;CC;;;WD)"),
        format!("D:(A;;CCDC;;;{a})"),
        format!("D:(A;OICI;CC;;;{a})"),
        "D:".to_owned(),
        "O:SYG:SY".to_owned(),
    ] {
        assert_eq!(sddl_users(&odd), None, "{odd}");
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
fn the_permit_opens_the_proxy_and_listed_ports_only() {
    let dir = TempDir::new("wfp-permit-ports");
    let listed = Loopback::Ports(vec![61150, 61091]);
    let ports = permit_ports(&policy(&dir, &[61090, 61091], listed)).unwrap();
    assert_eq!(ports, [61090, 61091, 61150]);
    let none = permit_ports(&policy(&dir, &[61090], Loopback::None)).unwrap();
    assert_eq!(none, [61090]);
    let any = permit_ports(&policy(&dir, &[61090], Loopback::Any));
    assert_eq!(any, Err(NetworkGap::LoopbackAny));
    let outside = permit_ports(&policy(&dir, &[61090], Loopback::Ports(vec![9222])));
    assert_eq!(outside, Err(NetworkGap::PortOutsideRange(9222)));
    assert_eq!(checked_ports(&[61179, 61080]), Ok(vec![61080, 61179]));
    assert_eq!(
        checked_ports(&[61180]),
        Err(NetworkGap::PortOutsideRange(61180))
    );
    assert_eq!(checked_ports(&[]), Ok(vec![]));
}

#[test]
fn the_permit_is_loopback_tcp_for_the_matters_user_above_every_block() {
    let ns = Namespace::production();
    let filters = permit_filters(&ns, &sid(2002), &[61090, 61150]);
    assert_eq!(filters.len(), 2);
    for (filter, port) in filters.iter().zip([61090, 61150]) {
        assert_eq!(filter.key, None, "a permit is dynamic");
        assert_eq!(filter.layer, Layer::ConnectV4);
        assert_eq!(filter.action, Action::Permit);
        assert!(filter.weight > BLOCK_WEIGHT && filter.weight == PERMIT_WEIGHT);
        assert_eq!(
            filter.conditions,
            [
                Condition::Users(vec![sid(2002)]),
                Condition::RemoteV4(Ipv4Addr::LOCALHOST),
                Condition::RemotePort(port),
                Condition::Protocol(PROTOCOL_TCP),
            ]
        );
    }
}

/// A change to an installed [`FakeWfp`], given the first and last block filter keys.
type Change = fn(&mut FakeWfp, u128, u128);

/// WFP in memory: the provider, the sublayer and filters by key; a read can fail.
struct FakeWfp {
    provider: bool,
    sublayer: bool,
    filters: BTreeMap<u128, FoundFilter>,
    fail: Option<(&'static str, u32)>,
}

impl FakeWfp {
    /// What `install` would leave for `pool` in `ns`.
    fn installed(ns: &Namespace, pool: &[Sid]) -> FakeWfp {
        let filters = block_filters(ns, pool)
            .into_iter()
            .map(|f| {
                let found = FoundFilter {
                    layer: Some(f.layer),
                    block: true,
                    sublayer: ns.sublayer,
                    users: Some(user_sddl(pool).replacen("O:SYG:SYD:", "D:", 1)),
                };
                (f.key.unwrap(), found)
            })
            .collect();
        FakeWfp {
            provider: true,
            sublayer: true,
            filters,
            fail: None,
        }
    }
}

impl SetupReader for FakeWfp {
    fn provider(&mut self, _key: u128) -> Result<bool, (&'static str, u32)> {
        self.fail.map_or(Ok(self.provider), Err)
    }

    fn sublayer(&mut self, _key: u128) -> Result<bool, (&'static str, u32)> {
        Ok(self.sublayer)
    }

    fn filter(&mut self, key: u128) -> Result<Option<FoundFilter>, (&'static str, u32)> {
        Ok(self.filters.get(&key).cloned())
    }
}

#[test]
fn a_launch_passes_only_a_whole_set_up_naming_its_user() {
    let ns = Namespace::production();
    let pool = [sid(2001), sid(2002)];
    let mut wfp = FakeWfp::installed(&ns, &pool);
    assert_eq!(check_setup(&ns, &sid(2002), &mut wfp), Ok(()));
    assert_eq!(
        check_setup(&ns, &sid(2003), &mut wfp),
        Err(NetworkGap::PoolUserNotCovered),
        "a pool user set-up hasn't covered yet"
    );
    assert_eq!(
        check_setup(&Namespace::test(), &sid(2002), &mut wfp),
        Err(NetworkGap::FilterMissing(
            "Clauseo sandbox (test) block_connect_v4".to_owned()
        )),
        "another namespace's keys"
    );
}

#[test]
fn each_missing_or_changed_object_refuses_the_launch() {
    let ns = Namespace::production();
    let pool = [sid(2001)];
    let user = sid(2001);
    let first = ns.filter_key(0);
    let last = ns.filter_key(5);
    let cases: [(&str, Change, NetworkGap); 7] = [
        (
            "no provider",
            |w, _, _| w.provider = false,
            NetworkGap::NotSetUp,
        ),
        (
            "no sublayer",
            |w, _, _| w.sublayer = false,
            NetworkGap::NotSetUp,
        ),
        (
            "a filter gone",
            |w, _, last| drop(w.filters.remove(&last)),
            NetworkGap::FilterMissing("Clauseo sandbox block_icmp_v6".to_owned()),
        ),
        (
            "a permit in a block's place",
            |w, first, _| w.filters.get_mut(&first).unwrap().block = false,
            NetworkGap::FilterMissing("Clauseo sandbox block_connect_v4".to_owned()),
        ),
        (
            "another layer",
            |w, first, _| w.filters.get_mut(&first).unwrap().layer = None,
            NetworkGap::FilterMissing("Clauseo sandbox block_connect_v4".to_owned()),
        ),
        (
            "another sublayer",
            |w, first, _| w.filters.get_mut(&first).unwrap().sublayer = 7,
            NetworkGap::FilterMissing("Clauseo sandbox block_connect_v4".to_owned()),
        ),
        (
            "a deny in the descriptor",
            |w, _, last| {
                let users = &mut w.filters.get_mut(&last).unwrap().users;
                *users = Some("D:(A;;CC;;;S-1-5-21-11-22-33-2001)(D;;CC;;;WD)".to_owned());
            },
            NetworkGap::PoolUserNotCovered,
        ),
    ];
    for (what, change, gap) in cases {
        let mut wfp = FakeWfp::installed(&ns, &pool);
        change(&mut wfp, first, last);
        assert_eq!(
            check_setup(&ns, &user, &mut wfp),
            Err(gap.clone()),
            "{what}"
        );
        assert!(gap.needs_setup(), "{what}");
    }
    let mut wfp = FakeWfp::installed(&ns, &pool);
    wfp.fail = Some(("FwpmProviderGetByKey0", 5));
    let unreadable = NetworkGap::Unreadable {
        call: "FwpmProviderGetByKey0",
        code: 5,
    };
    assert_eq!(check_setup(&ns, &user, &mut wfp), Err(unreadable));
}

#[test]
fn a_stale_set_up_needs_set_up_and_a_failed_permit_fails_the_launch() {
    let code = |cause: NetworkGap| Error::NetworkUnfiltered { cause }.code();
    assert_eq!(code(NetworkGap::NotSetUp), ErrorCode::SandboxSetupRequired);
    assert_eq!(
        code(NetworkGap::FilterMissing("x".to_owned())),
        ErrorCode::SandboxSetupRequired
    );
    assert_eq!(
        code(NetworkGap::PoolUserNotCovered),
        ErrorCode::SandboxSetupRequired
    );
    let unreadable = NetworkGap::Unreadable { call: "c", code: 5 };
    assert_eq!(code(unreadable), ErrorCode::SandboxSetupRequired);
    assert_eq!(code(NetworkGap::LoopbackAny), ErrorCode::AgentLaunchFailed);
    assert_eq!(
        code(NetworkGap::PortOutsideRange(1)),
        ErrorCode::AgentLaunchFailed
    );
    let failed = NetworkGap::PermitFailed {
        call: "FwpmFilterAdd0",
        code: 5,
    };
    assert_eq!(code(failed.clone()), ErrorCode::AgentLaunchFailed);
    assert_eq!(
        Error::NetworkUnfiltered { cause: failed }.to_string(),
        "the sandbox couldn't permit this agent's network (FwpmFilterAdd0 failed, error 0x5)"
    );
}

#[test]
fn each_firewall_trouble_is_named_for_firm_it() {
    use super::spec::{FIREWALL_UNDER_POLICY, FirewallTrouble};
    for code in [0x8007_0426, 0x8007_06D9, 0x8007_06BA] {
        assert_eq!(
            FirewallTrouble::from_call("x", code),
            FirewallTrouble::ServiceStopped
        );
    }
    let com = FirewallTrouble::from_call("Rules::Add", 0x8000_4005);
    assert_eq!(
        com.words(),
        "a firewall call failed (Rules::Add, error 0x80004005), so the second \
         layer of Clauseo's network protection is off; its network filters still enforce"
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
    use super::check::WfpError;
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
}
