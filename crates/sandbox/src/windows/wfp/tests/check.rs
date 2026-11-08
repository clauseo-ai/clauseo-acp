//! The launch's check against WFP in memory (S15 PR7): a whole set-up passes for each covered
//! pool user; each missing, changed or foreign filter, and each unreadable object, refuses.

use std::collections::BTreeMap;

use super::super::check::{
    FoundCondition, FoundFilter, Kind, NetworkGap, SetupReader, check_setup, classify,
};
use super::super::spec::{
    Action, Condition, FilterSpec, Match, Namespace, SUBLAYER_WEIGHT, not_pool_sddl,
    old_permit_filter, slot_filters, slot_key, user_sddl,
};
use super::{group, setup, sid, slot_of};
use crate::windows::sids::Sid;

/// A read's failure.
type Failure = (&'static str, u32);

/// `spec` as WFP would hold it after set-up wrote it.
pub(super) fn as_written(ns: &Namespace, spec: &FilterSpec) -> FoundFilter {
    let dacl = |sddl: String| sddl.replacen("O:SYG:SY", "", 1);
    let conditions = spec
        .conditions
        .iter()
        .map(|c| match c {
            Condition::Users(users) => FoundCondition::Users {
                matching: Match::Equal,
                sddl: dacl(user_sddl(users)),
            },
            Condition::NotPool { group, pool } => FoundCondition::Users {
                matching: Match::NotEqual,
                sddl: dacl(not_pool_sddl(group, pool)),
            },
            Condition::NotLoopback => FoundCondition::NotLoopback,
            Condition::Protocol(p) => FoundCondition::Protocol(*p),
            Condition::RemoteV4(a) => FoundCondition::RemoteV4(u32::from(*a)),
            Condition::RemotePorts(lo, hi) => FoundCondition::RemotePorts(*lo, *hi),
            Condition::LocalPorts(lo, hi) => FoundCondition::LocalPorts(*lo, *hi),
            Condition::NotUsers(users) => FoundCondition::Users {
                matching: Match::NotEqual,
                sddl: dacl(user_sddl(users)),
            },
        })
        .collect();
    FoundFilter {
        key: spec.key,
        id: 0,
        name: spec.name.clone(),
        layer: Some(spec.layer),
        action: Some(spec.action),
        weight: Some(spec.weight),
        flags: spec.flags(),
        other_flags: 0,
        provider: Some(ns.provider),
        conditions,
    }
}

/// WFP in memory: the provider, the sublayer and our sublayer's filters by key.
struct FakeWfp {
    provider: bool,
    sublayer: Option<u16>,
    filters: BTreeMap<u128, FoundFilter>,
    fail: Option<Failure>,
}

impl FakeWfp {
    /// What `install` leaves for `pool` in `ns`.
    fn installed(ns: &Namespace, pool: &[u32]) -> FakeWfp {
        let filters = setup(pool)
            .filters(ns)
            .iter()
            .map(|spec| (spec.key, as_written(ns, spec)))
            .collect();
        FakeWfp {
            provider: true,
            sublayer: Some(SUBLAYER_WEIGHT),
            filters,
            fail: None,
        }
    }

    fn filter(&mut self, key: u128) -> &mut FoundFilter {
        self.filters.get_mut(&key).unwrap()
    }
}

impl SetupReader for FakeWfp {
    fn provider(&mut self, _key: u128) -> Result<bool, Failure> {
        self.fail.map_or(Ok(self.provider), Err)
    }

    fn sublayer(&mut self, _key: u128) -> Result<Option<u16>, Failure> {
        Ok(self.sublayer)
    }

    fn filters_in(&mut self, _key: u128) -> Result<Vec<FoundFilter>, Failure> {
        Ok(self.filters.values().cloned().collect())
    }
}

#[test]
fn a_whole_set_up_passes_for_each_covered_user_only() {
    let ns = Namespace::production();
    let mut wfp = FakeWfp::installed(&ns, &[2001, 2002]);
    assert_eq!(check_setup(&ns, &sid(2001), &mut wfp), Ok(slot_of(2001)));
    assert_eq!(check_setup(&ns, &sid(2002), &mut wfp), Ok(slot_of(2002)));
    assert_eq!(
        check_setup(&ns, &sid(2003), &mut wfp),
        Err(NetworkGap::PoolUserNotCovered),
        "a pool user set-up hasn't covered yet"
    );
    let found: Vec<Kind> = wfp.filters.values().map(|f| classify(&ns, f)).collect();
    assert!(matches!(found[0], Kind::Block(0, _)));
    assert!(matches!(found[11], Kind::Block(11, _)));
    assert_eq!(found[12], Kind::Slot(slot_of(2001), 0, sid(2001)));
    assert_eq!(found[18], Kind::Slot(slot_of(2001), 6, sid(2001)));
    assert_eq!(found[19], Kind::Slot(slot_of(2002), 0, sid(2002)));
}

#[test]
fn another_namespace_s_filters_are_foreign() {
    let mut wfp = FakeWfp::installed(&Namespace::production(), &[2001]);
    let refused = check_setup(&Namespace::test(), &sid(2001), &mut wfp);
    assert_eq!(
        refused,
        Err(NetworkGap::ForeignFilter(
            "Clauseo sandbox block_connect_v4".to_owned()
        ))
    );
}

/// A change to an installed [`FakeWfp`]: the first block's key and the slot's first filter's.
type Change = fn(&mut FakeWfp, u128, u128);

#[test]
fn each_missing_changed_or_foreign_filter_refuses_the_launch() {
    let ns = Namespace::production();
    let connect = || NetworkGap::FilterMissing("Clauseo sandbox block_connect_v4".to_owned());
    let foreign = |name: &str| NetworkGap::ForeignFilter(name.to_owned());
    let cases: Vec<(&str, Change, NetworkGap)> = vec![
        (
            "no provider",
            |w, _, _| w.provider = false,
            NetworkGap::NotSetUp,
        ),
        (
            "no sublayer",
            |w, _, _| w.sublayer = None,
            NetworkGap::NotSetUp,
        ),
        (
            "a block gone",
            |w, f, _| drop(w.filters.remove(&f)),
            connect(),
        ),
        (
            "the slot's connect permit gone",
            |w, _, p| drop(w.filters.remove(&p)),
            NetworkGap::FilterMissing("Clauseo sandbox connect_host slot 17".to_owned()),
        ),
        (
            "a block turned permit",
            |w, f, _| w.filter(f).action = Some(Action::Permit),
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block without its cleared action right",
            |w, f, _| w.filter(f).flags.clear_action_right = false,
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block at another weight",
            |w, f, _| w.filter(f).weight = Some(15),
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block matching EQUAL",
            |w, f, _| {
                if let FoundCondition::Users { matching, .. } = &mut w.filter(f).conditions[0] {
                    *matching = Match::Equal;
                }
            },
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block with an extra condition",
            |w, f, _| w.filter(f).conditions.push(FoundCondition::Protocol(17)),
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block no longer denying the user",
            |w, f, _| {
                let p = "S-1-5-21-11-22-33";
                let sddl = format!(
                    "D:(D;;CC;;;{p}-3000)(A;;CC;;;WD)(A;;CC;;;RC)(A;;CC;;;AC)(A;;CC;;;S-1-15-2-2)"
                );
                w.filter(f).conditions[0] = FoundCondition::Users {
                    matching: Match::NotEqual,
                    sddl,
                };
            },
            NetworkGap::PoolUserNotCovered,
        ),
        (
            "a permit widened to every port",
            |w, _, p| w.filter(p).conditions[3] = FoundCondition::RemotePorts(1, 65535),
            foreign("Clauseo sandbox connect_host slot 17"),
        ),
        (
            "a permit for two users",
            |w, _, p| {
                let sddl = "D:(A;;CC;;;S-1-5-21-11-22-33-2001)(A;;CC;;;S-1-5-21-11-22-33-9)";
                w.filter(p).conditions[0] = FoundCondition::Users {
                    matching: Match::Equal,
                    sddl: sddl.to_owned(),
                };
            },
            foreign("Clauseo sandbox connect_host slot 17"),
        ),
        (
            "a permit that clears its action right",
            |w, _, p| w.filter(p).flags.clear_action_right = true,
            foreign("Clauseo sandbox connect_host slot 17"),
        ),
        (
            "someone else's filter in our sublayer",
            |w, _, p| {
                let mut stray = w.filters[&p].clone();
                stray.key = 7;
                stray.name = "Stray".to_owned();
                w.filters.insert(7, stray);
            },
            foreign("Stray"),
        ),
    ];
    refuses_each(&ns, cases);
}

/// Applies each change to a fresh set-up and checks it refuses with its gap.
fn refuses_each(ns: &Namespace, cases: Vec<(&str, Change, NetworkGap)>) {
    let (first, permit) = (ns.filter_key(0), slot_key(ns, slot_of(2001), 0));
    for (what, change, gap) in cases {
        let mut wfp = FakeWfp::installed(ns, &[2001]);
        change(&mut wfp, first, permit);
        assert_eq!(
            check_setup(ns, &sid(2001), &mut wfp),
            Err(gap.clone()),
            "{what}"
        );
        assert!(gap.needs_setup(), "{what}");
    }
}

#[test]
fn a_disabled_or_moved_filter_a_reweighted_sublayer_or_an_outside_permit_refuses() {
    let ns = Namespace::production();
    let foreign = |name: &str| NetworkGap::ForeignFilter(name.to_owned());
    let cases: Vec<(&str, Change, NetworkGap)> = vec![
        (
            "the sublayer at another weight",
            |w, _, _| w.sublayer = Some(1),
            NetworkGap::FilterMissing("Clauseo sandbox sublayer".to_owned()),
        ),
        (
            "a block disabled",
            |w, f, _| w.filter(f).other_flags = 0x20,
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block under another provider",
            |w, f, _| w.filter(f).provider = Some(7),
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a block with no provider",
            |w, f, _| w.filter(f).provider = None,
            foreign("Clauseo sandbox block_connect_v4"),
        ),
        (
            "a slot for a user outside the pool",
            |w, _, _| {
                let ns = Namespace::production();
                for spec in slot_filters(&ns, slot_of(2005), &sid(2999)) {
                    w.filters.insert(spec.key, as_written(&ns, &spec));
                }
            },
            foreign("Clauseo sandbox connect_host slot 21"),
        ),
        (
            "a second slot for the same user",
            |w, _, _| {
                let ns = Namespace::production();
                for spec in slot_filters(&ns, slot_of(2005), &sid(2001)) {
                    w.filters.insert(spec.key, as_written(&ns, &spec));
                }
            },
            foreign("Clauseo sandbox connect_host slot 21"),
        ),
        (
            "a slot filter naming another user than the rest",
            |w, _, p| {
                let sddl = "D:(A;;CC;;;S-1-5-21-11-22-33-2002)";
                w.filter(p).conditions[0] = FoundCondition::Users {
                    matching: Match::Equal,
                    sddl: sddl.to_owned(),
                };
            },
            foreign("Clauseo sandbox connect_pool slot 17"),
        ),
        (
            "an earlier set-up's shared permit left beside the slot's",
            |w, _, _| {
                let ns = Namespace::production();
                let spec = old_permit_filter(&ns, 0, &sid(2001));
                w.filters.insert(spec.key, as_written(&ns, &spec));
            },
            NetworkGap::OldSetup(
                "Clauseo sandbox permit loopback range S-1-5-21-11-22-33-2001".to_owned(),
            ),
        ),
    ];
    refuses_each(&ns, cases);
}

#[test]
fn an_unreadable_set_up_refuses_the_launch() {
    let ns = Namespace::production();
    let mut wfp = FakeWfp::installed(&ns, &[2001]);
    wfp.fail = Some(("FwpmProviderGetByKey0", 5));
    let unreadable = NetworkGap::Unreadable {
        call: "FwpmProviderGetByKey0",
        code: 5,
    };
    assert_eq!(check_setup(&ns, &sid(2001), &mut wfp), Err(unreadable));
}

#[test]
fn a_block_denying_a_well_known_sid_is_foreign() {
    let ns = Namespace::production();
    let mut wfp = FakeWfp::installed(&ns, &[2001]);
    let everyone = Sid::parse("S-1-5-32-545").unwrap();
    let spec = &setup(&[2001]).filters(&ns)[0];
    let mut odd = as_written(&ns, spec);
    odd.conditions[0] = FoundCondition::Users {
        matching: Match::NotEqual,
        sddl: not_pool_sddl(&group(), &[everyone]).replacen("O:SYG:SY", "", 1),
    };
    wfp.filters.insert(odd.key, odd);
    assert_eq!(
        check_setup(&ns, &sid(2001), &mut wfp),
        Err(NetworkGap::ForeignFilter(
            "Clauseo sandbox block_connect_v4".to_owned()
        ))
    );
}

#[test]
fn an_old_install_with_the_shared_permit_needs_set_up() {
    let ns = Namespace::production();
    let mut wfp = FakeWfp::installed(&ns, &[2001]);
    wfp.filters
        .retain(|_, f| !f.name.contains(" slot ") && !f.name.contains("listen"));
    let old = old_permit_filter(&ns, 0, &sid(2001));
    wfp.filters.insert(old.key, as_written(&ns, &old));
    assert_eq!(
        classify(&ns, &wfp.filters[&old.key]),
        Kind::Permit(sid(2001))
    );
    let refused = check_setup(&ns, &sid(2001), &mut wfp).unwrap_err();
    assert_eq!(
        refused,
        NetworkGap::OldSetup(
            "Clauseo sandbox permit loopback range S-1-5-21-11-22-33-2001".to_owned()
        )
    );
    assert!(refused.needs_setup());
    // Without the old permit, the missing listen blocks and slot filters still refuse.
    wfp.filters.remove(&old.key);
    let missing = check_setup(&ns, &sid(2001), &mut wfp).unwrap_err();
    assert!(
        matches!(missing, NetworkGap::FilterMissing(_)),
        "{missing:?}"
    );
    assert!(missing.needs_setup());
}
