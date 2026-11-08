//! Hardening the tasks that run as the lawyer (`harden.rs`): an administrator's, SYSTEM's or
//! `TrustedInstaller`'s task whose principal may be the lawyer loses exactly the write its DACL
//! gives an account a pool user may be, every other byte kept; a task anyone else owns, or that
//! runs as someone else, is untouched; nothing is ever added, and a failure is counted, never 56.

use super::super::harden::{STRIPPED, harden_loose_tasks};
use super::*;
use crate::windows::acl_bytes::{AceSpec, Acl, Descriptor, mask, sid_bytes};

/// `FILE_ALL_ACCESS`, `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE` (`FRFX`), `GENERIC_WRITE`.
const FA: u32 = 0x001F_01FF;
const FRFX: u32 = 0x0012_00A9;
const GW: u32 = 0x4000_0000;
/// `TrustedInstaller`.
const TI: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// A system audit entry's type, for a SACL that must stay.
const AUDIT: u8 = 2;

/// An entry of type `kind` with `flags`, `mask` and `sid`.
fn ace(kind: u8, flags: u8, mask: u32, sid: &str) -> Vec<u8> {
    let sid = sid_bytes(&Sid::parse(sid).unwrap());
    AceSpec {
        kind,
        flags,
        mask,
        sid,
    }
    .bytes()
}

/// An allow of `mask` for `sid`.
fn allow(mask: u32, sid: &str) -> Vec<u8> {
    ace(0, 0, mask, sid)
}

/// An ACL holding `aces`.
fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
    let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
    let mut out = vec![2, 0];
    out.extend_from_slice(&u16::try_from(size).unwrap().to_le_bytes());
    out.extend_from_slice(&u16::try_from(aces.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend(aces.iter().flatten());
    out
}

/// A self-relative descriptor owned by `owner`, its group SYSTEM, with `sacl` when given and a
/// DACL of `dacl`, laid out as hardening lays one out (owner, group, SACL, DACL).
fn described(owner: &str, sacl: Option<&[Vec<u8>]>, dacl: &[Vec<u8>]) -> Vec<u8> {
    let parts = [
        Some(sid_bytes(&Sid::parse(owner).unwrap())),
        Some(sid_bytes(&Sid::parse("S-1-5-18").unwrap())),
        sacl.map(acl),
        Some(acl(dacl)),
    ];
    // Self-relative, a DACL present, protected; a SACL present when given.
    let control: u16 = 0x9004 | if sacl.is_some() { 0x10 } else { 0 };
    let mut out = vec![1, 0];
    out.extend_from_slice(&control.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for (at, part) in parts.iter().enumerate() {
        if let Some(part) = part {
            let offset = u32::try_from(out.len()).unwrap().to_le_bytes();
            out[4 + 4 * at..8 + 4 * at].copy_from_slice(&offset);
            out.extend_from_slice(part);
        }
    }
    out
}

/// A descriptor owned by `owner` with `dacl`, no SACL.
fn descriptor(owner: &str, dacl: &[Vec<u8>]) -> Vec<u8> {
    described(owner, None, dacl)
}

/// A task's descriptor as the Task Scheduler leaves an administrator's: owned by
/// Administrators, all access for SYSTEM and Administrators, read and execute for Users.
pub(super) fn admin_owned() -> Vec<u8> {
    descriptor(
        "S-1-5-32-544",
        &[
            allow(FA, "S-1-5-18"),
            allow(FA, "S-1-5-32-544"),
            allow(FRFX, "S-1-5-32-545"),
        ],
    )
}

/// The DACL of a loose task: SYSTEM's and Administrators' all access, the lawyer's write, a
/// deny, Users given write alone (`GW`), Everyone all access, and Authenticated Users read.
fn loose_dacl() -> Vec<Vec<u8>> {
    vec![
        ace(1, 0, GW, "S-1-5-21-1-2-3-1500"),
        allow(FA, "S-1-5-18"),
        allow(FA, "S-1-5-32-544"),
        allow(FA, OWNER),
        allow(GW, "S-1-5-32-545"),
        allow(FA, "S-1-1-0"),
        allow(FRFX, "S-1-5-11"),
    ]
}

/// What hardening leaves of [`loose_dacl`]: Users' entry gone, Everyone's read and execute
/// alone; every other entry as it was, in order.
fn hardened_dacl() -> Vec<Vec<u8>> {
    vec![
        ace(1, 0, GW, "S-1-5-21-1-2-3-1500"),
        allow(FA, "S-1-5-18"),
        allow(FA, "S-1-5-32-544"),
        allow(FA, OWNER),
        allow(FA & !STRIPPED, "S-1-1-0"),
        allow(FRFX, "S-1-5-11"),
    ]
}

/// A fake of pool user 1010 whose task `\A\loose` runs as `principal` with `descriptor`.
fn loose(principal: Principal, descriptor: Vec<u8>) -> Fake {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "loose", principal);
    fake.descriptors.insert("loose".to_owned(), Ok(descriptor));
    fake
}

/// The run's one log line.
fn line(fake: &Fake) -> &str {
    &fake.logged[0].1
}

#[test]
fn a_loose_admin_owned_task_that_runs_as_the_lawyer_loses_only_the_offending_entries() {
    let sacl = [ace(AUDIT, 0x80, FA, "S-1-1-0")];
    let mut fake = loose(
        user(OWNER),
        described("S-1-5-32-544", Some(&sacl), &loose_dacl()),
    );
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    // Byte for byte: the owner, group, SACL and control as they were, and the DACL less
    // exactly the two entries.
    let want = described("S-1-5-32-544", Some(&sacl), &hardened_dacl());
    assert_eq!(fake.secured, [("loose".to_owned(), want)]);
    let line = line(&fake);
    assert!(
        line.ends_with(
            "; 1 tasks that run as the lawyer had standard users' write removed, 0 couldn't be \
             (named in the admin-only log)"
        ),
        "{line}"
    );
    assert!(
        !line.contains("S-1-"),
        "no SID in the world-readable line: {line}"
    );
    assert!(
        !line.contains("loose"),
        "no path in the world-readable line: {line}"
    );
    let admin = fake.admin_logged.join("\n");
    assert!(
        admin.contains(
            "the sandbox's task sweep hardened a task that runs as the lawyer, \"\\A\\loose\": \
             S-1-5-32-545 lost 0x40000000 (its entry removed); S-1-1-0 lost 0x000d0116"
        ),
        "{admin}"
    );
}

#[test]
fn a_loose_task_naming_a_group_the_lawyer_may_be_in_is_hardened() {
    // Users, and a local group whose members hold the lawyer.
    for named in ["S-1-5-32-545", "S-1-5-21-1-2-3-1200"] {
        let mut fake = loose(group(named), descriptor("S-1-5-18", &loose_dacl()));
        fake.groups.insert(sid(1200), Ok(vec![sid(1001)]));
        assert_eq!(sweep(&mut fake), SweepExit::Done, "{named}");
        let want = descriptor("S-1-5-18", &hardened_dacl());
        assert_eq!(fake.secured, [("loose".to_owned(), want)], "{named}");
    }
    // A local group that holds neither the lawyer nor a group that may: untouched.
    let mut fake = loose(
        group("S-1-5-21-1-2-3-1201"),
        descriptor("S-1-5-18", &loose_dacl()),
    );
    fake.groups.insert(sid(1201), Ok(vec![sid(1500)]));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured, []);
    // One whose members can't be read may hold the lawyer: hardened.
    let mut fake = loose(
        group("S-1-5-21-1-2-3-1202"),
        descriptor("S-1-5-18", &loose_dacl()),
    );
    fake.groups.insert(
        sid(1202),
        Err("NetLocalGroupGetMembers failed (5)".to_owned()),
    );
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured.len(), 1);
}

#[test]
fn a_task_the_lawyer_or_another_user_owns_is_untouched() {
    for owner in [OWNER, "S-1-5-21-1-2-3-1500"] {
        let mut fake = loose(user(OWNER), descriptor(owner, &loose_dacl()));
        assert_eq!(sweep(&mut fake), SweepExit::Done, "{owner}");
        assert_eq!(fake.secured, [], "{owner}");
        assert!(!line(&fake).contains("write removed"), "{}", line(&fake));
    }
}

#[test]
fn an_admin_owned_task_that_runs_as_another_user_or_system_is_untouched() {
    // Another local user (not a group), SYSTEM by its name, and LocalService.
    for principal in ["S-1-5-21-1-2-3-1500", "LocalSystem", "S-1-5-19"] {
        let mut fake = loose(user(principal), descriptor("S-1-5-32-544", &loose_dacl()));
        assert_eq!(sweep(&mut fake), SweepExit::Done, "{principal}");
        assert_eq!(fake.secured, [], "{principal}");
    }
}

#[test]
fn a_trusted_installer_task_is_hardened_only_when_it_runs_as_the_lawyer() {
    let mut fake = loose(user("S-1-5-18"), descriptor(TI, &loose_dacl()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.secured, []);
    let mut fake = loose(user(OWNER), descriptor(TI, &loose_dacl()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(
        fake.secured,
        [("loose".to_owned(), descriptor(TI, &hardened_dacl()))]
    );
}

#[test]
fn a_mixed_entry_keeps_its_read_rights_and_its_flags() {
    // Users: read, execute and write data, inherited; INTERACTIVE: read and WRITE_DAC.
    let mixed = [
        allow(FA, "S-1-5-18"),
        ace(0, 0x10, FRFX | 0x2, "S-1-5-32-545"),
        allow(0x0012_0089 | 0x4_0000, "S-1-5-4"),
    ];
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &mixed));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let want = [
        allow(FA, "S-1-5-18"),
        ace(0, 0x10, FRFX, "S-1-5-32-545"),
        allow(0x0012_0089, "S-1-5-4"),
    ];
    let want = descriptor("S-1-5-32-544", &want);
    assert_eq!(fake.secured, [("loose".to_owned(), want)]);
}

#[test]
fn hardening_never_adds_a_right_and_spares_the_lawyer_admins_and_denies() {
    let entries = [
        allow(FA, "S-1-5-18"),
        allow(FA, "S-1-5-32-544"),
        allow(FA, OWNER),
        allow(FA, "S-1-3-4"),
        allow(FA, "S-1-5-21-1-2-3-1500"),
        allow(FA, "S-1-5-32-551"),
        ace(1, 0, FA, "S-1-5-32-545"),
        allow(GW | 0x8000_0000, "S-1-5-32-545"),
        allow(0x1000_0000, "S-1-5-11"),
        allow(0x8_0000 | 0x1, "S-1-5-4"),
        allow(0x1_0000, "S-1-1-0"),
        allow(FA, "S-1-5-21-1-2-3-1100"),
        allow(FA, "S-1-5-21-1-2-3-1010"),
        allow(FA, "S-1-2-0"),
    ];
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &entries));
    fake.pool_group = Ok(Some(sid(1100)));
    // Backup Operators holds no pool user.
    fake.groups
        .insert(Sid::parse("S-1-5-32-551").unwrap(), Ok(Vec::new()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let [(_, written)] = fake.secured.as_slice() else {
        panic!("{:?}", fake.secured);
    };
    let dacl = Descriptor::parse(written).unwrap().dacl.unwrap();
    let dacl = Acl::parse(dacl).unwrap();
    let kept: Vec<Vec<u8>> = dacl.aces().iter().map(|ace| ace.to_vec()).collect();
    // Each entry kept is one of the originals, in order, with its mask the same or smaller.
    let mut from = entries.iter();
    for ace in &kept {
        let original = from
            .find(|original| original[..4] == ace[..4] && original[8..] == ace[8..])
            .unwrap_or_else(|| panic!("an entry that wasn't there: {ace:?}"));
        let (was, now) = (mask(original).unwrap(), mask(ace).unwrap());
        assert_eq!(now & !was, 0, "a right added: {was:#x} -> {now:#x}");
    }
    let want = [
        allow(FA, "S-1-5-18"),
        allow(FA, "S-1-5-32-544"),
        allow(FA, OWNER),
        allow(FA, "S-1-3-4"),
        allow(FA, "S-1-5-21-1-2-3-1500"),
        allow(FA, "S-1-5-32-551"),
        ace(1, 0, FA, "S-1-5-32-545"),
        allow(0x8000_0000, "S-1-5-32-545"),
        allow(0x1, "S-1-5-4"),
        // DELETE alone isn't a write: left.
        allow(0x1_0000, "S-1-1-0"),
        allow(FA & !STRIPPED, "S-1-5-21-1-2-3-1100"),
        allow(FA & !STRIPPED, "S-1-5-21-1-2-3-1010"),
        allow(FA & !STRIPPED, "S-1-2-0"),
    ];
    assert_eq!(kept, want);
}

#[test]
fn a_failed_write_is_counted_and_logged_and_never_56() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.secure_refuse.push("loose".to_owned());
    // An unreadable descriptor, and a NULL DACL, which hardening can't close.
    fake.task("\\A", "unread", user(OWNER));
    let unread = Err("IRegisteredTask::GetSecurityDescriptor failed (0x80070005)".to_owned());
    fake.descriptors.insert("unread".to_owned(), unread);
    fake.task("\\A", "null", user(OWNER));
    let mut null = descriptor("S-1-5-32-544", &[]);
    null[2] &= !0x04;
    fake.descriptors.insert("null".to_owned(), Ok(null));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let (error, line) = &fake.logged[0];
    assert!(!error, "{line}");
    assert!(
        line.contains(
            "; 0 tasks that run as the lawyer had standard users' write removed, 3 \
                       couldn't be"
        ),
        "{line}"
    );
    let admin = fake.admin_logged.join("\n");
    for name in ["loose", "unread", "null"] {
        let want = format!("couldn't harden a task that runs as the lawyer, \"\\A\\{name}\"");
        assert!(admin.contains(&want), "{admin}");
    }
    assert!(admin.contains("SetSecurityDescriptor failed"), "{admin}");
    assert!(
        admin.contains("it would have lost: S-1-5-32-545 lost"),
        "{admin}"
    );
}

#[test]
fn a_pool_owned_task_that_runs_as_the_lawyer_is_deleted_not_hardened() {
    let mut fake = loose(
        user(OWNER),
        descriptor("S-1-5-21-1-2-3-1010", &loose_dacl()),
    );
    fake.owners.insert(
        "loose".to_owned(),
        Ok("O:S-1-5-21-1-2-3-1010G:SYD:(A;;FA;;;SY)".to_owned()),
    );
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), ["\\A\\loose"]);
    assert_eq!(fake.secured, []);
}

#[test]
fn harden_loose_tasks_walks_every_folder_and_deletes_nothing() {
    let mut fake = loose(user(OWNER), descriptor("S-1-5-32-544", &loose_dacl()));
    fake.task("\\A\\B", "deep", group("S-1-1-0"));
    fake.descriptors.insert(
        "deep".to_owned(),
        Ok(descriptor("S-1-5-32-544", &loose_dacl())),
    );
    fake.task("\\A", "pool", user("S-1-5-21-1-2-3-1010"));
    let pool = Pool {
        owner: Sid::parse(OWNER).unwrap(),
        users: vec![sid(1010)],
    };
    let hardened = harden_loose_tasks(&mut fake, &pool);
    assert_eq!(
        (hardened.tasks, hardened.failed, hardened.unlisted),
        (2, 0, 0)
    );
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let mut secured: Vec<&str> = fake.secured.iter().map(|(name, _)| name.as_str()).collect();
    secured.sort_unstable();
    assert_eq!(secured, ["deep", "loose"]);
}
