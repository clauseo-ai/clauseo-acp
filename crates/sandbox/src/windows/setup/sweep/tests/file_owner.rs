//! A task the Task Scheduler won't give is held to its own owner before its file, and its file
//! counts only when its owner and DACL leave no pool user a way to have changed it: a pool
//! owner on the task is the pool's task, deleted; an owner that can't be read, a pool owner on
//! the file, a NULL DACL, or a right to change the file for anyone a pool user's token may hold
//! all stay 56; a clean file naming someone else is still skipped.

use super::task_file::{file, hidden};
use super::*;
use crate::windows::acl_bytes::{AceSpec, sid_bytes};

/// The rights [`WRITE_RIGHTS`] holds, one by one.
const RIGHTS: [u32; 9] = [
    0x2,         // FILE_WRITE_DATA
    0x4,         // FILE_APPEND_DATA
    0x10,        // FILE_WRITE_EA
    0x100,       // FILE_WRITE_ATTRIBUTES
    0x1_0000,    // DELETE
    0x4_0000,    // WRITE_DAC
    0x8_0000,    // WRITE_OWNER
    0x1000_0000, // GENERIC_ALL
    0x4000_0000, // GENERIC_WRITE
];

/// Read, execute and synchronize: no right to change the file.
const READ: u32 = 0x0012_00A9;

/// A fake of pool user 1010 whose hidden task `\A\B\t` has a file naming SYSTEM, its security
/// `security`.
fn system_file(security: FileSecurity) -> Fake {
    let mut fake = hidden("t", Ok(file("<UserId>S-1-5-18</UserId>")));
    fake.securities.insert(r"A\B\t".to_owned(), security);
    fake
}

/// [`clean`], with an allow of `mask` for `sid` (`None`: an allow whose SID wasn't read).
fn with_allow(sid: Option<&str>, mask: u32) -> FileSecurity {
    let mut security = clean();
    let sid = sid.map(|sid| Sid::parse(sid).unwrap());
    security.allows.as_mut().unwrap().push((sid, mask));
    security
}

/// Sweeps `fake`, which must end 56 with its task left and counted unreadable; the fake after.
fn stays_56(mut fake: Fake, case: &str) -> Fake {
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft, "{case}");
    assert_eq!(fake.deleted(), Vec::<&str>::new(), "{case}");
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 of the pool's tasks deleted, 1 unreadable (0 more skipped"),
        "{case}: {line}"
    );
    fake
}

#[test]
fn a_hidden_task_whose_own_owner_is_a_pool_user_is_deleted_whatever_its_file_says() {
    let mut fake = system_file(clean());
    let sddl = "O:S-1-5-21-1-2-3-1010G:SYD:(A;;FA;;;SY)";
    fake.owners.insert("t".to_owned(), Ok(sddl.to_owned()));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), [r"\A\B\t"]);
    // The owner decides before the file is read.
    assert!(!fake.done.iter().any(|done| done.starts_with("file ")));
}

#[test]
fn a_hidden_task_whose_own_owner_cant_be_read_stays_56_and_its_file_isnt_read() {
    for owner in [
        Err("IRegisteredTask::GetSecurityDescriptor failed (0x80070005)".to_owned()),
        Ok("G:SYD:(A;;FA;;;SY)".to_owned()),
    ] {
        let mut fake = system_file(clean());
        fake.owners.insert("t".to_owned(), owner.clone());
        let fake = stays_56(fake, &format!("{owner:?}"));
        assert!(!fake.done.iter().any(|done| done.starts_with("file ")));
    }
}

#[test]
fn a_task_file_a_pool_user_owns_stays_56() {
    let mut security = clean();
    security.owner = sid(1010);
    stays_56(system_file(security), "a pool owner");
}

#[test]
fn a_task_file_with_a_null_dacl_stays_56() {
    let mut security = clean();
    security.allows = None;
    stays_56(system_file(security), "a NULL DACL");
}

#[test]
fn a_right_to_change_the_task_file_for_anyone_a_pool_user_may_be_stays_56() {
    let who = [
        Some("S-1-5-32-545"),        // Users
        Some("S-1-5-11"),            // Authenticated Users
        Some("S-1-1-0"),             // Everyone
        Some("S-1-5-4"),             // INTERACTIVE
        Some("S-1-5-21-1-2-3-1010"), // the pool user
        Some("S-1-2-0"),             // LOCAL
        Some("S-1-5-113"),           // Local account
        Some("S-1-5-5-0-12345"),     // a logon session
        Some("S-1-5-21-1-2-3-513"),  // the machine's None group
        Some("S-1-5-21-1-2-3-1500"), // another group of the machine (the pool's)
        Some("S-1-5-32-551"),        // Backup Operators
        None,                        // an object allow, whose SID isn't read
    ];
    for sid in who {
        for right in RIGHTS {
            let fake = system_file(with_allow(sid, right | READ));
            stays_56(fake, &format!("{sid:?} {right:#x}"));
        }
    }
}

#[test]
fn a_clean_task_file_naming_someone_else_is_still_skipped() {
    // Full control for SYSTEM, Administrators, TrustedInstaller, the owner's stand-ins and
    // another domain's account; read alone for Users and the pool user.
    let mut security = clean();
    let allows = security.allows.as_mut().unwrap();
    for (sid, mask) in [
        (
            "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
            0x001F_01FF,
        ),
        ("S-1-3-4", 0x001F_01FF),
        ("S-1-5-21-9-9-9-1001", 0x001F_01FF),
        ("S-1-5-21-1-2-3-1010", READ),
        ("S-1-1-0", READ),
    ] {
        allows.push((Some(Sid::parse(sid).unwrap()), mask));
    }
    let mut fake = system_file(security);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.deleted(), Vec::<&str>::new());
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 unreadable (1 more skipped, not the pool's"),
        "{line}"
    );
}

/// A DACL entry: its type, SID and mask.
type Entry<'e> = (u8, &'e str, u32);

/// A descriptor's DACL, for [`descriptor`].
#[derive(Debug, Clone, Copy)]
enum Dacl<'d> {
    /// None: the DACL-present bit unset.
    Absent,
    /// A NULL DACL: present, at offset 0.
    Null,
    /// These entries.
    Of(&'d [Entry<'d>]),
}

/// A self-relative descriptor owned by `owner` with `dacl`.
fn descriptor(owner: Option<&str>, dacl: Dacl<'_>) -> Vec<u8> {
    let owner = owner.map(|owner| sid_bytes(&Sid::parse(owner).unwrap()));
    let entries = match dacl {
        Dacl::Of(entries) => Some(entries),
        Dacl::Absent | Dacl::Null => None,
    };
    let acl = entries.map(|entries| {
        let aces: Vec<u8> = entries
            .iter()
            .flat_map(|(kind, sid, mask)| {
                let sid = sid_bytes(&Sid::parse(sid).unwrap());
                let ace = AceSpec {
                    kind: *kind,
                    flags: 0,
                    mask: *mask,
                    sid,
                };
                ace.bytes()
            })
            .collect();
        let size = u16::try_from(8 + aces.len()).unwrap();
        let count = u16::try_from(entries.len()).unwrap();
        let mut acl = vec![2, 0];
        acl.extend(size.to_le_bytes());
        acl.extend(count.to_le_bytes());
        acl.extend([0, 0]);
        acl.extend(aces);
        acl
    });
    let present: u16 = if matches!(dacl, Dacl::Absent) {
        0
    } else {
        0x0004
    };
    let mut out = vec![1, 0];
    out.extend((0x8000 | present).to_le_bytes());
    out.extend([0; 16]);
    let at = |out: &Vec<u8>| u32::try_from(out.len()).unwrap().to_le_bytes();
    if let Some(owner) = owner {
        let offset = at(&out);
        out[4..8].copy_from_slice(&offset);
        out.extend(owner);
    }
    if let Some(acl) = acl {
        let offset = at(&out);
        out[16..20].copy_from_slice(&offset);
        out.extend(acl);
    }
    out
}

#[test]
fn the_task_files_security_is_read_from_its_descriptor() {
    let aces: &[Entry<'_>] = &[
        (0, "S-1-5-18", 0x001F_01FF),
        (1, "S-1-1-0", 0x2), // a deny: never counted
        (0, "S-1-5-32-545", READ),
        (5, "S-1-5-11", 0x2), // an object allow: its SID isn't read
    ];
    let security = file_security(&descriptor(Some("S-1-5-32-544"), Dacl::Of(aces))).unwrap();
    assert_eq!(security.owner, Sid::parse("S-1-5-32-544").unwrap());
    let allows = security.allows.clone().unwrap();
    let sids: Vec<_> = allows.iter().map(|(sid, _)| sid.clone()).collect();
    assert_eq!(
        sids,
        [
            Some(Sid::parse("S-1-5-18").unwrap()),
            Some(Sid::parse("S-1-5-32-545").unwrap()),
            None,
        ]
    );
    assert_eq!(allows[1].1, READ);
    assert!(trusted(&security, &[sid(1010)]).is_err());
    let clean: &[Entry<'_>] = &[(0, "S-1-5-18", 0x001F_01FF), (0, "S-1-5-32-545", READ)];
    let security = file_security(&descriptor(Some("S-1-5-32-544"), Dacl::Of(clean))).unwrap();
    assert_eq!(trusted(&security, &[sid(1010)]), Ok(()));
    // A NULL DACL and no DACL both let everyone in.
    for dacl in [Dacl::Null, Dacl::Absent] {
        let security = file_security(&descriptor(Some("S-1-5-18"), dacl)).unwrap();
        assert_eq!(security.allows, None, "{dacl:?}");
        assert!(trusted(&security, &[sid(1010)]).is_err());
    }
    // No owner, or a descriptor that isn't self-relative, can't be read.
    assert!(file_security(&descriptor(None, Dacl::Of(clean))).is_err());
    let mut absolute = descriptor(Some("S-1-5-18"), Dacl::Of(clean));
    absolute[3] = 0;
    assert!(file_security(&absolute).is_err());
}

#[test]
fn an_owner_is_read_only_from_a_revision_1_self_relative_descriptor() {
    use crate::windows::acl_bytes::owner_sid;

    // The owner a cache key's `SD` gives a hidden task, and a task file's.
    let good = descriptor(Some("S-1-5-18"), Dacl::Absent);
    assert_eq!(owner_sid(&good), Some(Sid::parse("S-1-5-18").unwrap()));
    let mut revision = good.clone();
    revision[0] = 2;
    let mut absolute = good;
    absolute[3] &= !0x80; // SE_SELF_RELATIVE cleared, the owner's offset left.
    for (bad, case) in [(revision, "revision 2"), (absolute, "not self-relative")] {
        assert_eq!(owner_sid(&bad), None, "{case}");
        assert!(file_security(&bad).is_err(), "{case}");
    }
    // An empty value names none either.
    assert_eq!(owner_sid(&[]), None);
}

#[test]
fn only_a_pool_users_possible_sids_count_as_open() {
    let pool = [sid(1010)];
    for (text, open) in [
        ("S-1-5-18", false),
        ("S-1-5-32-544", false),
        ("S-1-5-19", false),
        ("S-1-5-20", false),
        ("S-1-3-0", false),
        ("S-1-3-4", false),
        ("S-1-5-80-0", false),
        ("S-1-5-21-9-9-9-1001", false),
        ("S-1-5-21-1-2-3-1001", true),
        ("S-1-5-21-1-2-3-1010", true),
        ("S-1-5-32-545", true),
        ("S-1-5-6", true),
        ("S-1-15-2-1", true),
        // An Entra ID account or group (the lawyer's, say) is no pool user's.
        (
            "S-1-12-1-1111111111-2222222222-3333333333-1444444444",
            false,
        ),
        (
            "S-1-12-1-3555555555-1666666666-2777777777-3888888888",
            false,
        ),
        // Another S-1-12 form, or an unknown authority, may still be.
        ("S-1-12-2-1", true),
        ("S-1-99-1", true),
    ] {
        let sid = Sid::parse(text).unwrap();
        assert_eq!(may_be_pool(Some(&sid), &pool), open, "{text}");
    }
    assert!(may_be_pool(None, &pool));
    // A pool SID with no domain leaves every account under S-1-5-21 possible.
    let odd = [Sid::parse("S-1-5-99-1010").unwrap()];
    let other = Sid::parse("S-1-5-21-9-9-9-1001").unwrap();
    assert!(may_be_pool(Some(&other), &odd));
}
