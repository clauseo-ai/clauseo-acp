//! Each slot's filters (0049, "Loopback ports per slot"): their exact conditions, weights and
//! keys, the listen blocks, slots from account names, and what WFP's arbitration in our
//! sublayer makes of them for the traffic that matters (a small model of it: the matching
//! filter with the highest weight decides).

use std::net::Ipv4Addr;

use super::super::spec::{
    Action, BLOCK_WEIGHT, Condition, FilterSpec, Layer, Namespace, NetworkSetup, PERMIT_BASE,
    PERMIT_WEIGHT, PROTOCOL_TCP, SLOT_BASE, SLOT_STRIDE, SpecError, slot_filters, slot_key,
    slots_of,
};
use super::{group, owner, setup, sid, slot_of};
use crate::ports::{DAEMON_BLOCK, HOST_BLOCK, POOL_BLOCK, Slot};
use crate::windows::pool::PoolUser;
use crate::windows::sids::Sid;

#[test]
fn each_slot_gets_exactly_its_connect_and_listen_filters() {
    let ns = Namespace::production();
    let s = Slot::new(4).unwrap();
    let us = sid(2004);
    let filters = slot_filters(&ns, s, &us);
    let connect = |low: u16| {
        vec![
            Condition::Users(vec![us.clone()]),
            Condition::RemoteV4(Ipv4Addr::LOCALHOST),
            Condition::Protocol(PROTOCOL_TCP),
            Condition::RemotePorts(low, low + 7),
        ]
    };
    let listen = |low: u16| {
        vec![
            Condition::Users(vec![us.clone()]),
            Condition::LocalPorts(low, low + 7),
        ]
    };
    let only = vec![
        Condition::NotUsers(vec![us.clone()]),
        Condition::LocalPorts(46_388, 46_395),
    ];
    let expected: Vec<(&str, Layer, Action, u8, Vec<Condition>)> = vec![
        (
            "connect_host",
            Layer::ConnectV4,
            Action::Permit,
            PERMIT_WEIGHT,
            connect(46_132),
        ),
        (
            "connect_pool",
            Layer::ConnectV4,
            Action::Permit,
            PERMIT_WEIGHT,
            connect(46_388),
        ),
        (
            "connect_daemon",
            Layer::ConnectV4,
            Action::Permit,
            PERMIT_WEIGHT,
            connect(65_056),
        ),
        (
            "listen_pool",
            Layer::ListenV4,
            Action::Permit,
            PERMIT_WEIGHT,
            listen(46_388),
        ),
        (
            "listen_daemon",
            Layer::ListenV4,
            Action::Permit,
            PERMIT_WEIGHT,
            listen(65_056),
        ),
        (
            "only_listen_pool_v4",
            Layer::ListenV4,
            Action::Block,
            BLOCK_WEIGHT,
            only.clone(),
        ),
        (
            "only_listen_pool_v6",
            Layer::ListenV6,
            Action::Block,
            BLOCK_WEIGHT,
            only,
        ),
    ];
    assert_eq!(filters.len(), expected.len());
    for (n, (filter, (name, layer, action, weight, conditions))) in
        (0u16..).zip(filters.iter().zip(expected))
    {
        assert_eq!(filter.name, format!("Clauseo sandbox {name} slot 4"));
        assert_eq!(filter.key, ns.filter_key(SLOT_BASE + SLOT_STRIDE * 4 + n));
        assert_eq!(filter.key, slot_key(&ns, s, n));
        assert_eq!(
            (filter.layer, filter.action, filter.weight),
            (layer, action, weight),
            "{name}"
        );
        assert_eq!(filter.conditions, conditions, "{name}");
        assert!(filter.flags().persistent);
        assert_eq!(filter.flags().clear_action_right, action == Action::Block);
    }
    const { assert!(PERMIT_WEIGHT > BLOCK_WEIGHT) };
}

#[test]
fn the_listen_blocks_cover_every_slots_ports_for_every_pool_user_v4_and_v6() {
    let ns = Namespace::production();
    let filters = setup(&[2001, 2002]).filters(&ns);
    let listen: Vec<(&str, Layer, &[Condition])> = filters[6..12]
        .iter()
        .map(|f| (f.name.as_str(), f.layer, &f.conditions[1..]))
        .collect();
    let range = |r: &std::ops::RangeInclusive<u16>| [Condition::LocalPorts(*r.start(), *r.end())];
    let (h, p, d) = (range(&HOST_BLOCK), range(&POOL_BLOCK), range(&DAEMON_BLOCK));
    assert_eq!(
        listen,
        [
            (
                "Clauseo sandbox block_listen_host_v4",
                Layer::ListenV4,
                &h[..]
            ),
            (
                "Clauseo sandbox block_listen_host_v6",
                Layer::ListenV6,
                &h[..]
            ),
            (
                "Clauseo sandbox block_listen_pool_v4",
                Layer::ListenV4,
                &p[..]
            ),
            (
                "Clauseo sandbox block_listen_pool_v6",
                Layer::ListenV6,
                &p[..]
            ),
            (
                "Clauseo sandbox block_listen_daemon_v4",
                Layer::ListenV4,
                &d[..]
            ),
            (
                "Clauseo sandbox block_listen_daemon_v6",
                Layer::ListenV6,
                &d[..]
            ),
        ]
    );
    for filter in &filters[6..12] {
        assert_eq!(filter.action, Action::Block);
        assert_eq!(filter.weight, BLOCK_WEIGHT);
        let pool = Condition::NotPool {
            group: group(),
            pool: vec![sid(2001), sid(2002)],
        };
        assert_eq!(filter.conditions[0], pool);
    }
    // Nothing of ours sits at resource assignment but ICMP's raw sockets: a bind there would
    // also catch every outbound connect's source port.
    assert!(filters.iter().all(|f| {
        !matches!(f.layer, Layer::AssignV4 | Layer::AssignV6)
            || matches!(f.conditions.get(1), Some(Condition::Protocol(1 | 58)))
    }));
}

#[test]
fn the_slot_is_the_account_number_never_the_pool_position() {
    let user = |name: &str, rid: u32| PoolUser {
        name: name.to_owned(),
        sid: sid(rid),
    };
    let pool = [user("ClauseoSb07", 2001), user("ClauseoSb02", 2002)];
    let slots = slots_of(&pool).unwrap();
    assert_eq!(
        slots,
        [
            (Slot::new(6).unwrap(), sid(2001)),
            (Slot::new(1).unwrap(), sid(2002))
        ]
    );
    let set_up = NetworkSetup::new(&owner(), &group(), &slots).unwrap();
    // ClauseoSb02 keeps slot 1 when ClauseoSb01 is added before it.
    let grown = [user("ClauseoSb01", 2003), pool[0].clone(), pool[1].clone()];
    let after = NetworkSetup::new(&owner(), &group(), &slots_of(&grown).unwrap()).unwrap();
    let ns = Namespace::production();
    assert!(
        set_up
            .filters(&ns)
            .iter()
            .skip(12)
            .all(|f| after.filters(&ns).contains(f))
    );
    let stray = [user("ClauseoSbTC0a1b2c", 2004)];
    assert_eq!(
        slots_of(&stray),
        Err(SpecError::NoSlot("ClauseoSbTC0a1b2c".to_owned()))
    );
}

#[test]
fn two_users_in_one_slot_or_one_user_in_two_are_refused() {
    let s = |n: u8| Slot::new(n).unwrap();
    let new = |pool: &[(Slot, Sid)]| NetworkSetup::new(&owner(), &group(), pool);
    assert_eq!(
        new(&[(s(1), sid(2001)), (s(1), sid(2002))]),
        Err(SpecError::SlotClash(s(1)))
    );
    assert!(matches!(
        new(&[(s(1), sid(2001)), (s(2), sid(2001))]),
        Err(SpecError::SlotClash(_))
    ));
    assert!(new(&[(s(1), sid(2001)), (s(1), sid(2001))]).is_ok());
}

#[test]
fn removal_deletes_every_slots_keys_and_the_old_shared_permits() {
    let ns = Namespace::production();
    let keys = ns.removal_keys();
    for slot in Slot::all() {
        for filter in slot_filters(&ns, slot, &sid(2001)) {
            assert!(keys.contains(&filter.key), "{}", filter.name);
        }
    }
    assert!(keys.contains(&ns.filter_key(PERMIT_BASE)));
    assert!(keys.contains(&ns.filter_key(PERMIT_BASE + 31)));
    // The slots' keys clear the old permits' and the blocks'.
    const { assert!(PERMIT_BASE + 32 <= SLOT_BASE && 12 < PERMIT_BASE) };
}

/// One socket operation, as our sublayer sees it.
#[derive(Clone)]
struct Traffic {
    layer: Layer,
    user: Sid,
    pool_member: bool,
    remote: Option<Ipv4Addr>,
    remote_port: u16,
    local_port: u16,
}

/// Whether `condition` matches `t` (the user conditions as access checks of a plain token).
fn matches(condition: &Condition, t: &Traffic) -> bool {
    match condition {
        Condition::Users(users) => users.contains(&t.user),
        Condition::NotUsers(users) => !users.contains(&t.user),
        Condition::NotPool { .. } => t.pool_member,
        Condition::NotLoopback => false,
        Condition::Protocol(p) => *p == PROTOCOL_TCP,
        Condition::RemoteV4(address) => t.remote == Some(*address),
        Condition::RemotePorts(low, high) => (*low..=*high).contains(&t.remote_port),
        Condition::LocalPorts(low, high) => (*low..=*high).contains(&t.local_port),
    }
}

/// What our sublayer decides for `t`: the matching filter with the highest weight, a block
/// winning a tie; `None` when nothing matches (another sublayer decides).
fn decide(filters: &[FilterSpec], t: &Traffic) -> Option<Action> {
    filters
        .iter()
        .filter(|f| f.layer == t.layer && f.conditions.iter().all(|c| matches(c, t)))
        .max_by_key(|f| (f.weight, f.action == Action::Block))
        .map(|f| f.action)
}

#[test]
fn a_slot_connects_to_its_own_24_ports_on_127_0_0_1_and_nothing_else_on_loopback() {
    let ns = Namespace::production();
    let filters = setup(&[2001, 2002]).filters(&ns);
    let (ours, theirs) = (slot_of(2001), slot_of(2002));
    let connect = |user: Sid, address: Ipv4Addr, port: u16| Traffic {
        layer: Layer::ConnectV4,
        pool_member: user != owner(),
        user,
        remote: Some(address),
        remote_port: port,
        local_port: 50_000,
    };
    let local = Ipv4Addr::LOCALHOST;
    for port in ours.host().chain(ours.pool()).chain(ours.daemon()) {
        let permitted = decide(&filters, &connect(sid(2001), local, port));
        assert_eq!(permitted, Some(Action::Permit), "{port}");
    }
    let p = *ours.pool().start();
    let refused = [
        connect(sid(2001), local, *theirs.pool().start()),
        connect(sid(2001), local, *theirs.daemon().start()),
        connect(sid(2001), local, *theirs.host().start()),
        connect(sid(2001), local, 9222),
        connect(sid(2001), Ipv4Addr::new(127, 0, 0, 2), p),
        connect(sid(2001), Ipv4Addr::new(192, 168, 1, 5), p),
        connect(sid(2002), local, p),
        Traffic {
            layer: Layer::ConnectV6,
            remote: None,
            ..connect(sid(2001), local, p)
        },
    ];
    for traffic in &refused {
        assert_eq!(decide(&filters, traffic), Some(Action::Block));
    }
    assert_eq!(decide(&filters, &connect(owner(), local, 9222)), None);
}

#[test]
fn only_the_slots_user_listens_on_its_pool_ports_and_no_other_pool_user_on_its_daemon_ports() {
    let filters = setup(&[2001, 2002]).filters(&Namespace::production());
    let ours = slot_of(2001);
    let listen = |user: Sid, layer: Layer, port: u16| Traffic {
        layer,
        pool_member: user != owner() && user != sid(4000),
        user,
        remote: None,
        remote_port: 0,
        local_port: port,
    };
    let (v4, v6) = (Layer::ListenV4, Layer::ListenV6);
    let (p, d, h) = (
        *ours.pool().start(),
        *ours.daemon().end(),
        *ours.host().start(),
    );
    let (us, other, lawyer, service) = (sid(2001), sid(2002), owner(), sid(4000));
    let (permit, block) = (Some(Action::Permit), Some(Action::Block));
    let cases = [
        ("Us on its pool port", listen(us.clone(), v4, p), permit),
        ("Us on its daemon port", listen(us.clone(), v4, d), permit),
        (
            "Us on its pool port over IPv6",
            listen(us.clone(), v6, p),
            block,
        ),
        ("Us on its host port", listen(us.clone(), v4, h), block),
        (
            "Us on an unused slot's pool port",
            listen(us, v4, 46_600),
            block,
        ),
        (
            "another slot on Us's pool port",
            listen(other.clone(), v4, p),
            block,
        ),
        (
            "another slot on Us's daemon port",
            listen(other, v6, d),
            block,
        ),
        (
            "the lawyer on Us's pool port",
            listen(lawyer.clone(), v4, p),
            block,
        ),
        (
            "the lawyer on Us's pool port, v6",
            listen(lawyer.clone(), v6, p),
            block,
        ),
        (
            "the lawyer on Us's daemon port",
            listen(lawyer.clone(), v4, d),
            None,
        ),
        ("the lawyer on Us's host port", listen(lawyer, v4, h), None),
        ("a service on Us's pool port", listen(service, v4, p), block),
    ];
    for (what, traffic, expected) in cases {
        assert_eq!(decide(&filters, &traffic), expected, "{what}");
    }
}
