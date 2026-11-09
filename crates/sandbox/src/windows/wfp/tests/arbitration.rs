//! What WFP's arbitration in our sublayer makes of set-up's filters (0049, "Loopback ports per
//! slot"), for the tokens and traffic that matter: a small model of it. The matching filter with
//! the highest weight decides, a block winning a tie; nothing matching leaves it to the other
//! sublayers.
//!
//! An `ALE_USER_ID` condition is an access check of the socket's token against the condition's
//! descriptor for `FWP_ACTRL_MATCH_FILTER` (`CC`): `EQUAL` matches when it is granted,
//! `NOT_EQUAL` when it isn't. The model reads the real descriptors (`user_sddl`,
//! `not_pool_sddl`) and evaluates them the way `AccessCheck` does, as documented ("How DACLs
//! control access to an object", "Restricted tokens", the `AppContainer` access check). What it
//! assumes:
//! - the entries are read in order, and the first that names one of the token's SIDs decides,
//!   as every entry here asks for the one right `CC`;
//! - a deny entry names an enabled or a deny-only SID; an allow entry names an enabled SID only;
//! - a restricted token is checked twice, once with its user and groups and once with its
//!   restricting SIDs (all enabled), and is granted only if both grant;
//! - an `AppContainer` token is checked twice, once with its user and groups and once with its
//!   package SID, its capabilities and `ALL APPLICATION PACKAGES`, and is granted only if both
//!   grant; WFP applies that second check at `ALE_USER_ID` as `AccessCheck` does. The VM rows
//!   (`derived:denyonly`, `derived:lowbox`) measure the real outcome.

use std::net::Ipv4Addr;

use super::super::spec::{
    Action, Condition, FilterSpec, Layer, Namespace, PROTOCOL_TCP, not_pool_sddl, user_sddl,
};
use super::{group, owner, setup, sid, slot_of};
use crate::windows::sids::Sid;

const EVERYONE: &str = "S-1-1-0";
const RESTRICTED: &str = "S-1-5-12";
const USERS: &str = "S-1-5-32-545";
const ALL_PACKAGES: &str = "S-1-15-2-1";

fn well_known(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// A token, as an access check sees it.
#[derive(Clone)]
struct Token {
    /// Its user and groups, each with whether it is enabled (`false`: deny-only).
    sids: Vec<(Sid, bool)>,
    /// A restricted token's restricting SIDs.
    restricting: Option<Vec<Sid>>,
    /// An `AppContainer` token's package SID and capabilities.
    package: Option<Vec<Sid>>,
    /// Whether it holds a pool user's SID or the pool group's, enabled or deny-only.
    pool_member: bool,
}

impl Token {
    /// `user`'s plain token, with `Everyone` and `Users`; a pool user's also has the group.
    fn plain(user: &Sid) -> Token {
        let pool_member = *user != owner() && *user != sid(4000);
        let mut sids = vec![(user.clone(), true)];
        if pool_member {
            sids.push((group(), true));
        }
        sids.extend([(well_known(EVERYONE), true), (well_known(USERS), true)]);
        Token {
            sids,
            restricting: None,
            package: None,
            pool_member,
        }
    }

    /// The agent's token (`token.rs`): restricted, its restricting SIDs the pool user,
    /// `RESTRICTED`, `Users` and `Everyone`.
    fn agent(user: &Sid) -> Token {
        let restricting = [RESTRICTED, USERS, EVERYONE].map(well_known);
        Token {
            restricting: Some([vec![user.clone()], restricting.to_vec()].concat()),
            ..Token::plain(user)
        }
    }

    /// `user`'s token with its own user SID turned deny-only.
    fn deny_only(user: &Sid) -> Token {
        let mut token = Token::plain(user);
        token.sids[0].1 = false;
        token
    }

    /// `user`'s token restricted to `RESTRICTED` and `Everyone`, without its own SID.
    fn restricted_without_user(user: &Sid) -> Token {
        Token {
            restricting: Some([RESTRICTED, EVERYONE].map(well_known).to_vec()),
            ..Token::plain(user)
        }
    }

    /// `user`'s token in an `AppContainer` with no capability.
    fn app_container(user: &Sid) -> Token {
        let package = well_known("S-1-15-2-11-22-33-44-55-66-77");
        Token {
            package: Some(vec![package]),
            ..Token::plain(user)
        }
    }
}

/// A DACL's entries, from SDDL of `(<A|D>;;CC;;;<sid or alias>)` entries.
fn dacl(sddl: &str) -> Vec<(bool, Sid)> {
    let (_, entries) = sddl.split_once("D:").unwrap();
    entries
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(")(")
        .map(|entry| {
            let fields: Vec<&str> = entry.split(';').collect();
            let sid = match fields[5] {
                "WD" => EVERYONE,
                "RC" => RESTRICTED,
                "AC" => ALL_PACKAGES,
                other => other,
            };
            (fields[0] == "A", well_known(sid))
        })
        .collect()
}

/// One pass of an access check of `sids` against `entries`.
fn pass(entries: &[(bool, Sid)], sids: &[(Sid, bool)]) -> bool {
    for (allow, named) in entries {
        let held = sids.iter().find(|(sid, _)| sid == named);
        match held {
            Some((_, enabled)) if *allow && *enabled => return true,
            Some(_) if !*allow => return false,
            _ => {}
        }
    }
    false
}

/// Whether an access check of `token` against `sddl` grants `CC`.
fn granted(sddl: &str, token: &Token) -> bool {
    let entries = dacl(sddl);
    let enabled = |sids: &[Sid]| sids.iter().map(|s| (s.clone(), true)).collect::<Vec<_>>();
    let restricted = token.restricting.as_deref().map(enabled);
    let package = token
        .package
        .as_deref()
        .map(|sids| enabled(&[sids, &[well_known(ALL_PACKAGES)]].concat()));
    pass(&entries, &token.sids)
        && restricted.is_none_or(|sids| pass(&entries, &sids))
        && package.is_none_or(|sids| pass(&entries, &sids))
}

/// One socket operation, as our sublayer sees it.
#[derive(Clone)]
struct Traffic {
    layer: Layer,
    token: Token,
    remote: Option<Ipv4Addr>,
    remote_port: u16,
    local_port: u16,
}

/// Whether `condition` matches `t`.
fn matches(condition: &Condition, t: &Traffic) -> bool {
    match condition {
        Condition::Users(users) => granted(&user_sddl(users), &t.token),
        Condition::NotUsers(users) => !granted(&user_sddl(users), &t.token),
        Condition::NotPool { group, pool } => !granted(&not_pool_sddl(group, pool), &t.token),
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

fn connect(token: Token, address: Ipv4Addr, port: u16) -> Traffic {
    Traffic {
        layer: Layer::ConnectV4,
        token,
        remote: Some(address),
        remote_port: port,
        local_port: 50_000,
    }
}

fn listen(token: Token, layer: Layer, port: u16) -> Traffic {
    Traffic {
        layer,
        token,
        remote: None,
        remote_port: 0,
        local_port: port,
    }
}

#[test]
fn the_model_reads_pool_membership_as_the_blocks_descriptor_does() {
    let blocks = not_pool_sddl(&group(), &[sid(2001), sid(2002)]);
    for token in [
        Token::plain(&sid(2001)),
        Token::agent(&sid(2001)),
        Token::deny_only(&sid(2001)),
        Token::plain(&owner()),
        Token::app_container(&owner()),
    ] {
        assert_eq!(!granted(&blocks, &token), token.pool_member);
    }
}

#[test]
fn no_pool_token_connects_to_any_loopback_port_its_own_slots_included() {
    let filters = setup(&[2001, 2002]).filters(&Namespace::production());
    let (ours, theirs) = (slot_of(2001), slot_of(2002));
    let local = Ipv4Addr::LOCALHOST;
    let p = *ours.pool().start();
    let tokens = [
        Token::plain(&sid(2001)),
        Token::agent(&sid(2001)),
        Token::deny_only(&sid(2001)),
        Token::restricted_without_user(&sid(2001)),
        Token::app_container(&sid(2001)),
        Token::agent(&sid(2002)),
    ];
    for token in tokens {
        let ports = ours.host().chain(ours.pool()).chain(ours.daemon());
        for port in ports.chain([*theirs.pool().start(), *theirs.host().start(), 9222]) {
            let traffic = connect(token.clone(), local, port);
            assert_eq!(decide(&filters, &traffic), Some(Action::Block), "{port}");
        }
        for address in [Ipv4Addr::new(127, 0, 0, 2), Ipv4Addr::new(192, 168, 1, 5)] {
            let traffic = connect(token.clone(), address, p);
            assert_eq!(decide(&filters, &traffic), Some(Action::Block));
        }
        let v6 = Traffic {
            layer: Layer::ConnectV6,
            remote: None,
            ..connect(token.clone(), local, p)
        };
        assert_eq!(decide(&filters, &v6), Some(Action::Block));
    }
    let lawyer = connect(Token::plain(&owner()), local, p);
    assert_eq!(decide(&filters, &lawyer), None);
}

#[test]
fn only_the_slots_user_listens_on_its_pool_ports_and_no_other_pool_user_on_its_daemon_ports() {
    let filters = setup(&[2001, 2002]).filters(&Namespace::production());
    let ours = slot_of(2001);
    let (v4, v6) = (Layer::ListenV4, Layer::ListenV6);
    let (p, d, h) = (
        *ours.pool().start(),
        *ours.daemon().end(),
        *ours.host().start(),
    );
    let (us, other, lawyer, service) = (sid(2001), sid(2002), owner(), sid(4000));
    let plain = Token::plain;
    let (permit, block) = (Some(Action::Permit), Some(Action::Block));
    let cases = [
        ("Us on its pool port", listen(plain(&us), v4, p), permit),
        ("Us on its daemon port", listen(plain(&us), v4, d), permit),
        ("Us on its pool port, v6", listen(plain(&us), v6, p), permit),
        (
            "Us on its daemon port on [::]",
            listen(plain(&us), v6, d),
            permit,
        ),
        ("Us on its host port, v6", listen(plain(&us), v6, h), block),
        ("Us on its host port", listen(plain(&us), v4, h), block),
        (
            "Us on an unused slot's port",
            listen(plain(&us), v4, 46_600),
            block,
        ),
        (
            "another slot on Us's pool port",
            listen(plain(&other), v4, p),
            block,
        ),
        (
            "another slot on Us's daemon port",
            listen(plain(&other), v6, d),
            block,
        ),
        (
            "the lawyer on Us's pool port",
            listen(plain(&lawyer), v4, p),
            block,
        ),
        (
            "the lawyer on Us's pool port, v6",
            listen(plain(&lawyer), v6, p),
            block,
        ),
        (
            "the lawyer on Us's daemon port",
            listen(plain(&lawyer), v4, d),
            None,
        ),
        (
            "the lawyer on Us's host port",
            listen(plain(&lawyer), v4, h),
            None,
        ),
        (
            "a service on Us's pool port",
            listen(plain(&service), v4, p),
            block,
        ),
    ];
    for (what, traffic, expected) in cases {
        assert_eq!(decide(&filters, &traffic), expected, "{what}");
    }
}

#[test]
fn derived_tokens_keep_the_listen_permit_only_while_both_checks_grant_its_user() {
    let filters = setup(&[2001, 2002]).filters(&Namespace::production());
    let ours = slot_of(2001);
    let (p, d) = (*ours.pool().start(), *ours.daemon().start());
    let v4 = Layer::ListenV4;
    let (us, lawyer) = (sid(2001), owner());
    let (permit, block) = (Some(Action::Permit), Some(Action::Block));
    let cases = [
        // The agent's restricted token holds Us as a restricting SID: both passes grant it.
        (
            "the agent on its pool port",
            listen(Token::agent(&us), v4, p),
            permit,
        ),
        (
            "the agent on its daemon port",
            listen(Token::agent(&us), v4, d),
            permit,
        ),
        // A deny-only Us matches no allow, so the permit is lost; the denies still catch it.
        (
            "Us deny-only, pool port",
            listen(Token::deny_only(&us), v4, p),
            block,
        ),
        (
            "Us deny-only, daemon port",
            listen(Token::deny_only(&us), v4, d),
            block,
        ),
        // Restricting SIDs without Us: the second pass refuses the permit's allow.
        (
            "Us restricted without itself, pool port",
            listen(Token::restricted_without_user(&us), v4, p),
            block,
        ),
        (
            "Us restricted without itself, daemon port",
            listen(Token::restricted_without_user(&us), v4, d),
            block,
        ),
        // An `AppContainer`'s package pass finds no allow for its package: the permit is lost.
        (
            "Us in an AppContainer, pool port",
            listen(Token::app_container(&us), v4, p),
            block,
        ),
        (
            "Us in an AppContainer, daemon port",
            listen(Token::app_container(&us), v4, d),
            block,
        ),
        // The lawyer's derived tokens: still off P_s; on D_s the blocks' allows of RESTRICTED
        // and ALL APPLICATION PACKAGES keep them out of the pool's block.
        (
            "the lawyer restricted, pool port",
            listen(Token::restricted_without_user(&lawyer), v4, p),
            block,
        ),
        (
            "the lawyer restricted, daemon port",
            listen(Token::restricted_without_user(&lawyer), v4, d),
            None,
        ),
        (
            "the lawyer in an AppContainer, pool port",
            listen(Token::app_container(&lawyer), v4, p),
            block,
        ),
        (
            "the lawyer in an AppContainer, daemon port",
            listen(Token::app_container(&lawyer), v4, d),
            None,
        ),
        (
            "the lawyer deny-only, pool port",
            listen(Token::deny_only(&lawyer), v4, p),
            block,
        ),
    ];
    for (what, traffic, expected) in cases {
        assert_eq!(decide(&filters, &traffic), expected, "{what}");
    }
}
