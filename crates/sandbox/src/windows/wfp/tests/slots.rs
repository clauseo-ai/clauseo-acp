//! Each slot's filters (0049, "Loopback ports per slot"): their exact conditions, weights and
//! keys, the listen blocks, and slots from account names. What WFP's arbitration makes of them
//! is in `arbitration.rs`.

use super::super::spec::{
    Action, BLOCK_WEIGHT, Condition, Layer, Namespace, NetworkSetup, PERMIT_BASE, PERMIT_WEIGHT,
    RETIRED_OFFSETS, SLOT_BASE, SLOT_FIRST, SLOT_STRIDE, SpecError, slot_filters, slot_key,
    slot_offset_key, slots_of,
};
use super::{group, owner, setup, sid};
use crate::ports::{DAEMON_BLOCK, HOST_BLOCK, POOL_BLOCK, Slot};
use crate::windows::pool::PoolUser;
use crate::windows::sids::Sid;

#[test]
fn each_slot_gets_exactly_its_listen_filters_and_no_connect_permit() {
    let ns = Namespace::production();
    let s = Slot::new(4).unwrap();
    let us = sid(2004);
    let filters = slot_filters(&ns, s, &us);
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
        (
            "listen_pool_v6",
            Layer::ListenV6,
            Action::Permit,
            PERMIT_WEIGHT,
            listen(46_388),
        ),
        (
            "listen_daemon_v6",
            Layer::ListenV6,
            Action::Permit,
            PERMIT_WEIGHT,
            listen(65_056),
        ),
    ];
    assert_eq!(filters.len(), expected.len());
    for (n, (filter, (name, layer, action, weight, conditions))) in
        (0u16..).zip(filters.iter().zip(expected))
    {
        assert_eq!(filter.name, format!("Clauseo sandbox {name} slot 4"));
        // The keys an earlier build's connect permits had (offsets 0 to 2) stay unused.
        assert_eq!(
            filter.key,
            ns.filter_key(SLOT_BASE + SLOT_STRIDE * 4 + 3 + n)
        );
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
    const { assert!(PERMIT_WEIGHT > BLOCK_WEIGHT && SLOT_FIRST == 3) };
    // No static filter of set-up's permits a connect (PR6's SYSTEM server adds them).
    let all = setup(&[2001, 2002]).filters(&ns);
    assert!(all.iter().all(|f| {
        !matches!(f.layer, Layer::ConnectV4 | Layer::ConnectV6) || f.action == Action::Block
    }));
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
fn removal_deletes_every_slots_keys_the_retired_ones_and_the_old_shared_permits() {
    let ns = Namespace::production();
    let keys = ns.removal_keys();
    for slot in Slot::all() {
        for filter in slot_filters(&ns, slot, &sid(2001)) {
            assert!(keys.contains(&filter.key), "{}", filter.name);
        }
        // An earlier build of this branch's static connect permits.
        for offset in RETIRED_OFFSETS {
            assert!(keys.contains(&slot_offset_key(&ns, slot, offset)));
        }
    }
    assert!(keys.contains(&ns.filter_key(PERMIT_BASE)));
    assert!(keys.contains(&ns.filter_key(PERMIT_BASE + 31)));
    // The slots' keys clear the old permits' and the blocks'.
    const { assert!(PERMIT_BASE + 32 <= SLOT_BASE && 12 < PERMIT_BASE) };
}
