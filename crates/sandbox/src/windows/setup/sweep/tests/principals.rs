//! How the sweep matches a principal: a SID's text in any form Windows takes, a name through the
//! local lookup, a name that resolves to no SID by a pool user's name, with no prefix or this
//! computer's, and a task's owner. Another domain's name that resolves to no SID is skipped;
//! any other such name, no principal, or a principal or owner that can't be read is 56.

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
fn another_domains_unresolved_name_is_skipped_logged_and_not_56() {
    let mut fake = named();
    // A domain account with no domain controller reachable, and pool names under other
    // domains.
    for (at, name) in [r"CORP\alice", r"CORP\ClauseoSb01", r"pc-8\ClauseoSb02"]
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
        line.contains("0 unresolved (no SID, not another domain's), 3 skipped (another"),
        "{line}"
    );
    for name in ["alice", "CORP", "ClauseoSb", "pc-8"] {
        assert!(!line.contains(name), "{name}: {line}");
    }
}

#[test]
fn any_other_unresolved_name_is_56_and_the_pools_other_tasks_still_go() {
    // A near name, an empty prefix or account, a deleted account, and a prefix of two parts.
    for name in [
        "ClauseoSb011",
        "ClauseoSb",
        r"\ClauseoSb01",
        r"PC-7\",
        "deleted-user",
        r"CORP\x\alice",
    ] {
        let mut fake = named();
        fake.task("\\", "other", user(name));
        fake.task("\\", "pool", group("S-1-5-21-1-2-3-1010"));
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{name}");
        assert_eq!(fake.deleted(), ["\\pool"], "{name}");
        let (error, line) = &fake.logged[0];
        assert!(error);
        assert!(line.contains("1 unresolved (no SID"), "{name}: {line}");
        assert!(!line.contains(name), "{name}: {line}");
    }
    // With this computer's names unknown, no prefix is another domain's.
    let mut fake = named();
    fake.computer = Vec::new();
    fake.task("\\", "unknown-computer", user(r"CORP\alice"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
}

#[test]
fn a_principal_that_cant_be_read_is_still_56() {
    let mut fake = named();
    fake.unread("\\", "unread");
    fake.task("\\", "skipped", user(r"CORP\alice"));
    fake.task("\\", "pool", user("ClauseoSb02"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), ["\\pool"]);
    let line = &fake.logged[0].1;
    assert!(line.contains("1 unreadable (0 more skipped"), "{line}");
    assert!(line.contains("1 skipped"), "{line}");
}

#[test]
fn a_task_with_no_principal_is_56_unless_its_owner_is_the_pools() {
    let mut fake = named();
    fake.task("\\", "neither", Principal::default());
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let line = &fake.logged[0].1;
    assert!(line.contains("1 with no user or group"), "{line}");
    let mut fake = named();
    fake.task("\\", "neither", Principal::default());
    let owner = Ok("O:S-1-5-21-1-2-3-1011G:SY".to_owned());
    fake.owners.insert("neither".to_owned(), owner);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\neither"]);
}

#[test]
fn a_sids_text_in_lowercase_or_with_a_hex_authority_is_that_sid() {
    let mut fake = named();
    fake.task("\\", "lower", user("s-1-5-21-1-2-3-1010"));
    fake.task("\\", "hex", user("S-1-0x5-21-1-2-3-1011"));
    fake.task("\\", "hex-upper", group("s-1-0X000000000005-21-1-2-3-1010"));
    fake.task("\\", "zeros", user("S-01-5-021-1-2-3-01011"));
    fake.task("\\", "other", user("s-1-0x5-21-1-2-3-1012"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(
        fake.deleted(),
        ["\\lower", "\\hex", "\\hex-upper", "\\zeros"]
    );
    assert_eq!(fake.lookups(), Vec::<&str>::new());
}

#[test]
fn only_sid_text_windows_takes_is_canonicalised() {
    for (text, want) in [
        ("S-1-5-18", "S-1-5-18"),
        ("s-1-5-32-545", "S-1-5-32-545"),
        ("S-1-0xFFFFFFFFFFFF-1", "S-1-281474976710655-1"),
        ("S-1-0x10-0004294967295", "S-1-16-4294967295"),
    ] {
        assert_eq!(canonical_sid(text).unwrap().as_str(), want, "{text}");
    }
    for text in [
        "S-2-5-18",
        "S-1-5",
        "S-1-0x1000000000000-1",
        "S-1-5-4294967296",
        "S-1-5--18",
        "S-1-5-18-",
        "S-1-5-+18",
        "S-1-0x-18",
        "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
        "SY",
        "ClauseoSb01",
        "",
    ] {
        assert_eq!(canonical_sid(text), None, "{text}");
    }
}

#[test]
fn a_task_whose_owner_is_a_pool_sid_is_deleted_whatever_its_principal() {
    let mut fake = named();
    let lawyer = Sid::parse(OWNER).unwrap();
    fake.accounts.insert("lawyer".to_owned(), lawyer);
    fake.task("\\", "users", group("S-1-5-32-545"));
    fake.task("\\", "lawyer", user("lawyer"));
    fake.task("\\", "theirs", user("S-1-5-18"));
    for (task, sddl) in [
        ("users", "O:s-1-5-21-1-2-3-1010D:"),
        ("lawyer", "O:S-1-5-21-1-2-3-1011"),
        ("theirs", "O:SYG:SYD:(A;;FA;;;SY)"),
    ] {
        fake.owners.insert(task.to_owned(), Ok(sddl.to_owned()));
    }
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\users", "\\lawyer"]);
}

#[test]
fn an_owner_that_cant_be_read_is_56_and_the_task_is_left() {
    for sddl in [
        Err("IRegisteredTask::GetSecurityDescriptor failed (0x80070005)".to_owned()),
        Ok("G:SYD:(A;;FA;;;SY)".to_owned()),
        Ok("O:G:SY".to_owned()),
        Ok("O:ClauseoSb01".to_owned()),
    ] {
        let mut fake = named();
        fake.task("\\", "unowned", user("S-1-5-18"));
        fake.owners.insert("unowned".to_owned(), sddl.clone());
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{sddl:?}");
        assert_eq!(fake.deleted(), Vec::<&str>::new());
        let line = &fake.logged[0].1;
        assert!(line.contains("1 unreadable"), "{line}");
    }
    assert_eq!(sddl_owner("O:BAG:SY"), Ok(None));
    let owner = sddl_owner("O:S-1-5-21-1-2-3-1010G:S-1-5-18");
    assert_eq!(owner, Ok(Some(sid(1010))));
}

#[test]
fn a_pool_user_whose_name_cant_be_read_is_still_matched_by_its_sid() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.names.insert(sid(1010), "ClauseoSb01".to_owned());
    fake.task("\\", "by-sid", user("S-1-5-21-1-2-3-1011"));
    fake.task("\\", "by-name", user("ClauseoSb02"));
    fake.task("\\", "known", user("ClauseoSb01"));
    // Its name, unread and unresolved, may still be the pool's: left, and 56.
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), ["\\by-sid", "\\known"]);
    assert!(
        fake.logged[0].1.contains("1 unresolved"),
        "{:?}",
        fake.logged
    );
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
