//! Group-principal tasks (`group.rs`): judged by the owner their descriptor names, never by
//! the group they name or the `Author` they give. A pool owner is deleted (56 when it can't
//! be), an owner that can't be read is 56, any other owner is ignored; whether the group may
//! hold a pool user is counted for the log alone, and a lookup that fails counts as yes.

use super::task_file::file;
use super::*;

/// A task's descriptor naming pool user 1010 its owner.
const POOL_OWNED: &str = "O:S-1-5-21-1-2-3-1010G:SYD:(A;;FA;;;SY)";

/// A principal naming `group`, and no user, with `author`.
fn by(group: &str, author: &str) -> Principal {
    Principal {
        group_id: group.to_owned(),
        author: author.to_owned(),
        ..Principal::default()
    }
}

/// The run's one log line.
fn line(fake: &Fake) -> &str {
    &fake.logged[0].1
}

/// What the run wrote to the admin-only log: the paths, and authors, of what keeps it at 56.
fn admin(fake: &Fake) -> &str {
    &fake.admin_logged[0]
}

/// The groups whose members were looked up, in order.
fn members_read(fake: &Fake) -> Vec<&str> {
    let read = fake.done.iter().filter_map(|l| l.strip_prefix("members "));
    read.collect()
}

#[test]
fn a_group_task_owned_by_a_pool_user_is_deleted_whatever_the_group() {
    // Backup Operators, Everyone, Users and Administrators.
    for named in ["S-1-5-32-551", "S-1-1-0", "S-1-5-32-545", "S-1-5-32-544"] {
        let mut fake = Fake::new(&[1010]);
        fake.task("\\A", "grouped", group(named));
        fake.owners
            .insert("grouped".to_owned(), Ok(POOL_OWNED.to_owned()));
        assert_eq!(sweep(&mut fake), SweepExit::Done, "{named}");
        assert_eq!(fake.deleted(), ["\\A\\grouped"], "{named}");
        assert!(
            line(&fake).contains("1 group tasks deleted, 0 ignored"),
            "{}",
            line(&fake)
        );
    }
}

#[test]
fn a_pool_owned_group_task_that_cant_be_deleted_is_56() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "grouped", group("S-1-1-0"));
    fake.owners
        .insert("grouped".to_owned(), Ok(POOL_OWNED.to_owned()));
    fake.delete_refuse.push("grouped".to_owned());
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let line = line(&fake);
    assert!(line.contains("0 group tasks deleted, 0 ignored"), "{line}");
    assert!(line.contains("1 left (1 naming a group"), "{line}");
    assert!(
        admin(&fake).contains("at 56: \"\\A\\grouped\""),
        "{:?}",
        fake.admin_logged
    );
}

#[test]
fn a_group_task_whose_owner_cant_be_read_is_56() {
    let unreadable = Err("IRegisteredTask::GetSecurityDescriptor failed (0x80070005)".to_owned());
    // No owner in the SDDL at all, and an owner that isn't a SID or an alias.
    let ownerless = Ok("G:SYD:(A;;FA;;;BA)".to_owned());
    let odd = Ok("O:<x>G:SYD:(A;;FA;;;BA)".to_owned());
    for sddl in [unreadable, ownerless, odd] {
        let mut fake = Fake::new(&[1010]);
        fake.task("\\A", "grouped", group("S-1-5-32-551"));
        fake.owners.insert("grouped".to_owned(), sddl.clone());
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{sddl:?}");
        assert_eq!(fake.deleted(), Vec::<&str>::new());
        let line = line(&fake);
        assert!(line.contains("0 group tasks deleted, 0 ignored"), "{line}");
        assert!(line.contains(", 1 left ("), "{line}");
        assert!(
            admin(&fake).contains("at 56: \"\\A\\grouped\""),
            "{:?}",
            fake.admin_logged
        );
    }
}

#[test]
fn an_admin_owned_users_task_and_a_system_owned_sandbox_group_task_are_ignored() {
    let mut fake = Fake::new(&[1010]);
    let group_sid = sid(1100);
    fake.pool_group = Ok(Some(group_sid.clone()));
    fake.accounts
        .insert("ClauseoSandboxUsers".to_owned(), group_sid.clone());
    fake.groups.insert(group_sid, Ok(vec![sid(1010)]));
    // Owned by Administrators, the fake's default.
    fake.task("\\A", "users", group("S-1-5-32-545"));
    fake.task("\\A", "sandbox", group("ClauseoSandboxUsers"));
    fake.owners.insert(
        "sandbox".to_owned(),
        Ok("O:SYG:SYD:(A;;FA;;;SY)".to_owned()),
    );
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let line = line(&fake);
    assert!(
        line.contains(
            "0 group tasks deleted, 2 ignored (not owned by a pool user), 0 left (2 naming a \
             group a pool user may be in)"
        ),
        "{line}"
    );
    // Both groups are known to hold pool users without a members lookup; the sandbox group's
    // members are read once, for whether it may hold the lawyer (`harden.rs`).
    assert_eq!(members_read(&fake), ["S-1-5-21-1-2-3-1100"]);
}

#[test]
fn whether_a_group_may_hold_a_pool_user_is_read_once_and_a_failed_read_counts_as_yes() {
    let mut fake = Fake::new(&[1010]);
    let interactive = Sid::parse("S-1-5-4").unwrap();
    // 1200's members can't be read; 1201 holds the lawyer alone; 1202 a pool user; 1203
    // INTERACTIVE, which a pool user's session holds.
    fake.groups.insert(
        sid(1200),
        Err("NetLocalGroupGetMembers failed (5)".to_owned()),
    );
    fake.groups.insert(sid(1201), Ok(vec![sid(1001)]));
    fake.groups.insert(sid(1202), Ok(vec![sid(1010)]));
    fake.groups.insert(sid(1203), Ok(vec![interactive]));
    for (name, rid) in [
        ("a", 1200),
        ("b", 1200),
        ("c", 1201),
        ("d", 1202),
        ("e", 1203),
    ] {
        fake.task("\\A", name, group(&format!("S-1-5-21-1-2-3-{rid}")));
    }
    // Administrators: no pool user's token holds it, so no lookup.
    fake.task("\\A", "admins", group("S-1-5-32-544"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let line = line(&fake);
    assert!(
        line.contains("6 ignored (not owned by a pool user), 0 left (4 naming a group"),
        "{line}"
    );
    assert_eq!(
        members_read(&fake),
        [
            "S-1-5-21-1-2-3-1200",
            "S-1-5-21-1-2-3-1201",
            "S-1-5-21-1-2-3-1202",
            "S-1-5-21-1-2-3-1203"
        ]
    );
}

#[test]
fn the_author_has_no_effect_and_is_logged_cleaned_for_a_task_left_at_56() {
    let authors = [
        "",
        "NT AUTHORITY\\SYSTEM",
        "PC-7\\ClauseoSb01",
        "S-1-5-21-1-2-3-1010",
    ];
    for author in authors {
        let mut fake = Fake::new(&[1010]);
        fake.names.insert(sid(1010), "ClauseoSb01".to_owned());
        fake.task("\\A", "pool", by("S-1-1-0", author));
        fake.owners
            .insert("pool".to_owned(), Ok(POOL_OWNED.to_owned()));
        fake.task("\\A", "admins", by("S-1-5-32-545", author));
        fake.task("\\A", "unread", by("S-1-5-32-545", author));
        fake.owners.insert(
            "unread".to_owned(),
            Err("the descriptor can't be read".to_owned()),
        );
        assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{author}");
        assert_eq!(fake.deleted(), ["\\A\\pool"], "{author}");
        let line = line(&fake);
        assert!(
            line.contains("1 group tasks deleted, 1 ignored (not owned by a pool user), 1 left"),
            "{line}"
        );
        let label = "the authors group tasks left at 56 give (their registrant's word, never \
                     trusted)";
        // The world-readable line names no author; the admin-only log does, cleaned.
        assert!(!line.contains(label), "{line}");
        let admin = admin(&fake);
        if author.is_empty() {
            assert!(!admin.contains(label), "{admin}");
        } else {
            let cleaned = super::super::log_path::clean(author);
            assert!(admin.ends_with(&format!("; {label}: {cleaned}")), "{admin}");
            assert!(!admin.contains("S-1-5-21-1-2-3-1010"), "no SID: {admin}");
        }
    }
}

#[test]
fn a_user_principal_naming_a_group_is_a_group_task_and_the_service_accounts_arent() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "users", user("S-1-5-32-545"));
    fake.task("\\A", "local", user("S-1-5-21-1-2-3-1201"));
    fake.groups.insert(sid(1201), Ok(Vec::new()));
    fake.task("\\A", "system", user("S-1-5-18"));
    fake.task("\\A", "lawyer", user(OWNER));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let line = line(&fake);
    assert!(
        line.contains(
            "0 group tasks deleted, 2 ignored (not owned by a pool user), 0 left (1 \
                       naming a group"
        ),
        "{line}"
    );
}

/// A fake of pool user 1010 with `\A\hidden` hidden from the listing, its trusted task file
/// naming `group`.
fn hidden_group(group: &str) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.hidden.push(r"\A\hidden".to_owned());
    let named = file(&format!("<GroupId>{group}</GroupId>"));
    fake.files.insert(r"A\hidden".to_owned(), Ok(named));
    fake
}

#[test]
fn a_hidden_group_task_is_56_whatever_its_owner_or_group() {
    // Administrators, which no pool user's token holds, Users, which every one does, and a
    // local group whose members can't be read.
    for group in ["S-1-5-32-544", "S-1-5-32-545", "S-1-5-21-1-2-3-1200"] {
        // A readable owner that isn't a pool SID (Administrators, the fake's default), and an
        // owner that can't be read.
        let owners = [
            Ok(Sid::parse("S-1-5-32-544").unwrap()),
            Err("a task's descriptor in the Task Scheduler's cache names no owner".to_owned()),
        ];
        for owner in owners {
            let mut fake = hidden_group(group);
            fake.cache_owners
                .insert(r"\A\hidden".to_owned(), owner.clone());
            assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{group}: {owner:?}");
            assert_eq!(fake.deleted(), Vec::<&str>::new());
            let line = line(&fake);
            assert!(line.contains("1 not listed"), "{line}");
            assert!(line.contains("0 group tasks deleted, 0 ignored"), "{line}");
            assert!(
                admin(&fake).ends_with("at 56: \"\\A\\hidden\""),
                "{:?}",
                fake.admin_logged
            );
            assert_eq!(members_read(&fake), Vec::<&str>::new());
        }
    }
}
