//! The walk's basics through the fake: only a task whose principal resolves to exactly a pool
//! SID is deleted, the service names map to their SIDs, and an unreadable principal is 56
//! while the pool's other tasks still go.

use super::*;

#[test]
fn only_tasks_whose_principal_is_exactly_a_pool_sid_are_deleted() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.accounts.insert("ClauseoSb01".to_owned(), sid(1011));
    fake.accounts
        .insert("lawyer".to_owned(), Sid::parse(OWNER).unwrap());
    fake.accounts.insert("ClauseoSb1".to_owned(), sid(10111));
    // A pool SID by its text, and one by the account's name.
    fake.task("\\", "pool-sid", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\", "pool-name", user("ClauseoSb01"));
    // A SID a suffix or a prefix away from a pool SID, and a name that resolves to one.
    fake.task("\\", "longer", user("S-1-5-21-1-2-3-10101"));
    fake.task("\\", "shorter", user("S-1-5-21-1-2-3-101"));
    fake.task("\\", "near-name", user("ClauseoSb1"));
    // The lawyer's, SYSTEM's in each of its spellings, and the Users group's.
    fake.task("\\", "lawyer", user("lawyer"));
    fake.task("\\", "system", user("S-1-5-18"));
    fake.task("\\", "local-system", user("localsystem"));
    fake.task("\\", "network", user("NETWORKSERVICE"));
    fake.task("\\", "users", group("S-1-5-32-545"));
    // A group that is a pool SID; a user that isn't wins over a group that is.
    fake.task("\\", "pool-group", group("S-1-5-21-1-2-3-1011"));
    let both = Principal {
        user_id: "lawyer".to_owned(),
        group_id: "S-1-5-21-1-2-3-1010".to_owned(),
        ..Principal::default()
    };
    fake.task("\\", "user-first", both);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(
        fake.deleted(),
        ["\\pool-sid", "\\pool-name", "\\pool-group"]
    );
    // A SID's text and the service names never reach the lookup; each name is looked up once.
    assert_eq!(fake.lookups(), ["ClauseoSb01", "ClauseoSb1", "lawyer"]);
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert_eq!(
        line,
        "the sandbox's task sweep: 2 pool users, 1 folders and 12 tasks read, 3 of the pool's \
         tasks deleted, 0 unreadable (0 more skipped, not the pool's by their task file), 0 with \
         no user or group, 0 unresolved (no SID, not another domain's), 0 skipped (another \
         domain's, no SID), 0 not deleted, 0 not listed (in a task file or the cache only), 1 \
         group tasks deleted, 1 ignored (not owned by a pool user), 0 left (2 naming a group a \
         pool user may be in), 0 running instances stopped, 0 not, 0 BITS jobs cancelled, 0 not"
    );
}

#[test]
fn the_service_names_map_to_their_sids_in_any_case_and_nothing_else_does() {
    let mut fake = Fake::new(&[1010]);
    let names = Names::default();
    for (name, want) in [
        ("LocalSystem", "S-1-5-18"),
        ("LOCALSERVICE", "S-1-5-19"),
        ("networkservice", "S-1-5-20"),
        ("S-1-5-21-1-2-3-1010", "S-1-5-21-1-2-3-1010"),
    ] {
        let sid = Sid::parse(want).unwrap();
        assert_eq!(resolve(&mut fake, &names, name), Resolved::Sid(sid));
    }
    assert_eq!(fake.lookups(), Vec::<&str>::new());
    // Anything else is looked up, a near spelling or a domain-qualified name too; with no
    // computer name known, none is another domain's.
    for name in ["Local System", r"NT AUTHORITY\LocalSystem", "SYSTEM"] {
        let found = resolve(&mut fake, &names, name);
        assert!(matches!(found, Resolved::Unresolved(_)), "{name}");
    }
    assert_eq!(fake.lookups().len(), 3);
}

#[test]
fn an_unreadable_principal_is_56_and_the_pools_other_tasks_still_go() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "first", user("S-1-5-21-1-2-3-1010"));
    fake.unread("\\", "unread");
    fake.unread("\\", "unread-too");
    fake.task("\\", "last", group("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(SweepExit::TasksLeft.as_code(), 56);
    assert_eq!(fake.deleted(), ["\\first", "\\last"]);
    // The BITS step still ran, and then the pool tasks' instances were listed.
    let tail: Vec<&str> = fake.done.iter().rev().take(2).map(String::as_str).collect();
    assert_eq!(tail, ["running", "jobs"]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(
        line.contains("2 of the pool's tasks deleted, 2 unreadable"),
        "{line}"
    );
}
