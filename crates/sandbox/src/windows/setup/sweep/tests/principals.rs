//! How the sweep matches a principal: a SID's text directly, a name through the local lookup,
//! and a name that resolves to no SID only by a pool user's name, with no prefix or this
//! computer's; anything else such is skipped, not 55. A principal that can't be read still is.

use super::*;

/// A fake of pool users 1010 and 1011, named `ClauseoSb01` and `ClauseoSb02`.
fn named() -> Fake {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.names.insert(sid(1010), "ClauseoSb01".to_owned());
    fake.names.insert(sid(1011), "ClauseoSb02".to_owned());
    fake
}

#[test]
fn a_sid_is_compared_as_it_is_and_never_looked_up() {
    let mut fake = named();
    fake.task("\\", "pool", user("S-1-5-21-1-2-3-1011"));
    fake.task("\\", "other", user("S-1-5-21-1-2-3-1012"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\pool"]);
    assert_eq!(fake.lookups(), Vec::<&str>::new());
}

#[test]
fn a_name_that_resolves_is_held_to_its_sid_not_its_name() {
    let mut fake = named();
    // The pool user's own name resolves to its SID; a pool user's name that resolves to another
    // account's SID (a domain account of that name, say) isn't the pool's.
    fake.accounts.insert("ClauseoSb01".to_owned(), sid(1010));
    fake.accounts
        .insert(r"CORP\ClauseoSb02".to_owned(), sid(5000));
    fake.task("\\", "pool", user("ClauseoSb01"));
    fake.task("\\", "domain", user(r"CORP\ClauseoSb02"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\pool"]);
}

#[test]
fn an_unresolved_pool_name_is_the_pools_with_no_prefix_or_this_computers() {
    let mut fake = named();
    for (at, name) in [
        "ClauseoSb01",
        r".\clauseosb02",
        r"PC-7\CLAUSEOSB01",
        r"pc-7.firm.example\ClauseoSb02",
    ]
    .into_iter()
    .enumerate()
    {
        fake.task("\\", &format!("pool-{at}"), user(name));
    }
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(
        fake.deleted(),
        ["\\pool-0", "\\pool-1", "\\pool-2", "\\pool-3"]
    );
    assert!(fake.logged[0].1.contains("0 skipped"), "{:?}", fake.logged);
}

#[test]
fn any_other_unresolved_name_is_skipped_logged_and_not_55() {
    let mut fake = named();
    // A domain account with no domain controller reachable, a pool name under another domain,
    // a near name, an empty prefix or account, and a deleted account.
    for (at, name) in [
        r"CORP\alice",
        r"CORP\ClauseoSb01",
        "ClauseoSb011",
        "ClauseoSb",
        r"\ClauseoSb01",
        r"PC-7\",
        "deleted-user",
    ]
    .into_iter()
    .enumerate()
    {
        fake.task("\\", &format!("other-{at}"), user(name));
    }
    fake.task("\\", "pool", group("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\pool"]);
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert!(
        line.contains("0 unreadable, 7 skipped (no SID, not a pool name)"),
        "{line}"
    );
    for name in ["alice", "CORP", "ClauseoSb", "deleted-user", "PC-7"] {
        assert!(!line.contains(name), "{name}: {line}");
    }
}

#[test]
fn a_principal_that_cant_be_read_is_still_55() {
    let mut fake = named();
    fake.unread("\\", "unread");
    fake.task("\\", "skipped", user(r"CORP\alice"));
    fake.task("\\", "pool", user("ClauseoSb02"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), ["\\pool"]);
    assert!(
        fake.logged[0].1.contains("1 unreadable, 1 skipped"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn a_pool_user_whose_name_cant_be_read_is_still_matched_by_its_sid() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.names.insert(sid(1010), "ClauseoSb01".to_owned());
    fake.task("\\", "by-sid", user("S-1-5-21-1-2-3-1011"));
    fake.task("\\", "by-name", user("ClauseoSb02"));
    fake.task("\\", "known", user("ClauseoSb01"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\by-sid", "\\known"]);
}

#[test]
fn the_name_rule_ignores_case_and_only_this_computers_prefixes() {
    let names = Names {
        users: vec!["ClauseoSb07".to_owned()],
        computer: vec!["PC-7".to_owned()],
    };
    for name in ["clauseosb07", r".\CLAUSEOSB07", r"pc-7\ClauseoSb07"] {
        assert!(names.is_pool_name(name), "{name}");
    }
    for name in [
        r"PC-8\ClauseoSb07",
        r"..\ClauseoSb07",
        r"PC-7\x\ClauseoSb07",
        "ClauseoSb07 ",
        "",
    ] {
        assert!(!names.is_pool_name(name), "{name}");
    }
}
