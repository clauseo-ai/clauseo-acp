//! `WindowsMachine`'s account, group, hidden-user and record methods on a real Windows. The
//! unelevated tests run in CI's ordinary step; the `elevated_` ones only in its elevated step
//! (`#[ignore]`, and skipped at run time unless the token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`, D-B16). They use names with this family's own prefix
//! ([`FAMILY`]) and a random suffix, and the key `HKLM\SOFTWARE\Clauseo\SandboxTestAcct`, never
//! the real pool or record, and each first clears what a killed earlier try of this family left
//! (split §8), by prefix.

use std::io;
use std::path::{Path, PathBuf};

use super::account_list::names_starting;
use super::accounts::{self, Kind};
use super::lock_tests::{suffix, unique_mutex};
use super::machine::{Machine, MachineError, Password};
use super::owner::Owner;
use super::password::is_strong;
use super::record_values::RECORD_SDDL;
use super::record_write::{self, create_key_with, key_sddl};
use super::windows_machine::{Names, WindowsMachine};
use crate::test_support::TempDir;
use crate::windows::FolderSupport;
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::marker::MarkerState;
use crate::windows::pool::MachineRecord;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::elevated_tests_enabled;

/// This test family's record key.
const TEST_KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestAcct";

/// The start of every account and group name this test family makes, and only this family:
/// `ClauseoAcct`, then `U` (user) or `G` (group), then six hex digits: 18 characters.
const FAMILY: &str = "ClauseoAcct";

/// Surroundings for a machine whose fence never runs here.
pub(super) struct NoSurroundings;

impl FolderList for NoSurroundings {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for NoSurroundings {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn set_aside(&mut self, _matter: clauseo_protocol::MatterId, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

/// Deletes every local account and group whose name starts with [`FAMILY`], then the test
/// key: what a killed earlier try left (an account made before its record write included), or
/// this try's own at the end. Only this family's names: other test families have their own
/// prefixes and sweep their own. Each step goes on past a failure.
fn sweep() {
    for (kind, delete) in [
        (
            Kind::User,
            accounts::delete_user as fn(&str) -> Result<(), MachineError>,
        ),
        (Kind::Group, accounts::delete_group),
    ] {
        for name in names_starting(FAMILY, kind).unwrap_or_default() {
            drop(delete(&name));
        }
    }
    drop(record_write::delete(TEST_KEY));
}

/// Sweeps when dropped, so a failed assertion still cleans up.
struct Sweep;

impl Drop for Sweep {
    fn drop(&mut self) {
        sweep();
    }
}

#[test]
fn the_record_reader_answers_on_a_machine_with_no_key() {
    let missing = format!("SOFTWARE\\Clauseo\\NoSuchKey{}", suffix());
    assert_eq!(record_write::read(&missing), Ok(None));
    assert_eq!(record_write::delete(&missing), Ok(()));
    // The real key: absent on a runner. Read only.
    let mut real = read_only_machine();
    assert!(real.read_record().is_ok());
    assert_eq!(crate::windows::sandbox_pool_sids().ok(), Some(vec![]));
}

#[test]
fn lookups_answer_for_missing_and_non_local_accounts() {
    let mut machine = read_only_machine();
    let nobody = format!("{FAMILY}U{}", suffix());
    assert_eq!(machine.account_sid(&nobody), Ok(None));
    // SYSTEM is a well-known account of NT AUTHORITY, not a local user.
    let system = Sid::parse("S-1-5-18").unwrap();
    assert_eq!(machine.account_name(&system), Ok(None));
    // This user: a local account on the runner, or a domain one elsewhere (then `None`).
    let me = current_user_sid().unwrap();
    if let Some(name) = machine.account_name(&me).unwrap() {
        assert_eq!(machine.account_sid(&name), Ok(Some(me)));
    }
    assert_ne!(accounts::users_group().unwrap(), "");
    // A name that qualifies itself with a computer or domain is refused, not looked up.
    assert!(machine.account_sid("BUILTIN\\Users").is_err());
}

/// A machine on `names` for this process's user, whose data folder is `data_dir`.
pub(super) fn machine_on(names: Names, data_dir: &Path) -> WindowsMachine {
    let owner = Owner::of_this_process().unwrap();
    WindowsMachine::new(names, owner, data_dir.to_owned(), Box::new(NoSurroundings))
}

/// A machine on the real names whose data folder is never written.
fn read_only_machine() -> WindowsMachine {
    machine_on(Names::production(), Path::new(r"C:\ClauseoNoSuchData"))
}

#[test]
fn the_wired_methods_answer_as_this_user() {
    let dir = TempDir::new("windows-machine-wired");
    let data = dir.path().join("data");
    let mut machine = machine_on(Names::production(), &data);
    assert!(machine.seclogon().is_ok());
    assert!(machine.logon_right().is_ok());
    assert!(machine.program_data().unwrap().join("Microsoft").is_dir());
    assert_eq!(machine.owner(), &current_user_sid().unwrap());
    // The owner's files, each as the owner: none yet, then an empty secrets.bin.
    assert_eq!(machine.read_marker(), MarkerState::Missing);
    assert_eq!(machine.sessions_open(), Ok(false));
    assert_eq!(machine.ledger(&[]).unwrap().entries().count(), 0);
    machine.protect_for_owner(&[], &[]).unwrap();
    assert_eq!(
        crate::windows::secrets::read(&data).unwrap().sids().count(),
        0
    );
    // Inside `as_owner`, again: no impersonation is nested, and the thread is itself after.
    let inner = machine
        .as_owner(|m| m.as_owner(Machine::read_marker))
        .unwrap();
    assert_eq!(inner, Ok(MarkerState::Missing));
    let sid = Sid::parse("S-1-5-21-7-8-9-1010").unwrap();
    assert_eq!(machine.profile_of(&sid), Ok(None));
    assert!(
        machine.delete_profile(&sid).is_err(),
        "not this machine's pool user"
    );
    assert!(machine.now() > 1_700_000_000);
    assert!(is_strong(&machine.new_password()));
}

#[test]
fn as_owner_reverts_after_a_return_an_error_a_nested_call_and_a_panic() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::owner::thread_has_token;

    let dir = TempDir::new("windows-machine-as-owner");
    let mut machine = machine_on(Names::production(), dir.path());
    assert!(!thread_has_token());
    // A return: acting as the owner inside, itself after.
    assert_eq!(machine.as_owner(|_| thread_has_token()), Ok(true));
    assert!(!thread_has_token());
    // An error from inside: still reverted.
    let failed: Result<Result<(), MachineError>, _> =
        machine.as_owner(|_| Err(MachineError::Other("a step failed".to_owned())));
    assert!(matches!(failed, Ok(Err(MachineError::Other(_)))));
    assert!(!thread_has_token());
    // Nested: the inner call runs as the owner without a second impersonation, and the outer
    // one still acts as the owner after it returns.
    let nested = machine.as_owner(|m| {
        let inner = m.as_owner(|_| thread_has_token());
        (inner, thread_has_token())
    });
    assert_eq!(nested, Ok((Ok(true), true)));
    assert!(!thread_has_token());
    // A panic: the guard reverts during the unwind.
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        machine.as_owner(|_| panic!("a write failed while acting as the owner"))
    }));
    assert!(unwound.is_err());
    assert!(!thread_has_token(), "reverted during the unwind");
    assert_eq!(
        machine.as_owner(|_| thread_has_token()),
        Ok(true),
        "usable again"
    );
}

#[test]
#[ignore = "elevated: CI's elevated step runs it (CLAUSEO_ELEVATED_TESTS=1)"]
fn elevated_accounts_group_hidden_user_and_record() {
    if !elevated_tests_enabled() {
        return;
    }
    sweep();
    let _sweep = Sweep;
    let tag = suffix();
    let user = format!("{FAMILY}U{tag}");
    let names = Names {
        record_key: TEST_KEY.to_owned(),
        group: format!("{FAMILY}G{tag}"),
        mutex: unique_mutex().unwrap(),
        users: format!("{FAMILY}U"),
        journals: false,
    };
    assert!(user.len() <= 20 && names.group.len() <= 20, "{user}");
    let data = TempDir::new("windows-machine-accounts");
    let mut machine = machine_on(names, data.path());
    machine.lock().unwrap();

    // A key already there, owned by this user with an open DACL: the first write takes it back.
    // The SDDL text may name this user by an alias rather than its SID string (`LA` for the
    // built-in Administrator, which the hosted runner's account is), so the check is that the
    // owner is not Administrators and the open entry is there.
    let me = current_user_sid().unwrap();
    create_key_with(TEST_KEY, &format!("O:{me}D:(A;;KA;;;WD)")).unwrap();
    let planted = key_sddl(TEST_KEY).unwrap();
    assert!(
        !planted.starts_with("O:BA") && planted.contains(";;;WD)"),
        "{planted}"
    );

    // The group.
    assert_eq!(machine.group(), Ok(None));
    let group = machine.create_group().unwrap();
    assert_eq!(machine.group(), Ok(Some(group.clone())));
    assert!(matches!(
        machine.create_group(),
        Err(MachineError::Os {
            call: "NetLocalGroupAdd",
            code: 2223 | 1379
        })
    ));
    let owner = current_user_sid().unwrap();
    let mut record = MachineRecord {
        owner: owner.clone(),
        group: group.clone(),
        pool: vec![],
        data_dir: PathBuf::from(r"C:\ClauseoAcct\data"),
        removing: false,
    };
    machine.write_record(&record).unwrap();
    assert_eq!(machine.read_record(), Ok(Some(record.clone())));
    let taken = key_sddl(TEST_KEY).unwrap();
    assert!(taken.starts_with("O:BAD:P"), "{taken}");
    assert!(!taken.contains(";;;WD)"), "{taken}");

    // The user: made once, never adopted.
    let password = machine.new_password();
    assert!(is_strong(&password));
    let sid = machine.create_user(&user, &password).unwrap();
    record.pool.push(sid.clone());
    machine.write_record(&record).unwrap();
    let again = machine.create_user(&user, &password);
    assert_eq!(
        again,
        Err(MachineError::Os {
            call: "NetUserAdd",
            code: 2224
        })
    );
    assert_eq!(machine.account_sid(&user), Ok(Some(sid.clone())));
    assert_eq!(machine.account_name(&sid), Ok(Some(user.clone())));

    // Its groups, twice over; the group has exactly it.
    machine.add_to_group(&user, &group).unwrap();
    machine.add_to_group(&user, &group).unwrap();
    assert_eq!(machine.group_members(&group), Ok(1));
    let stranger = Sid::parse("S-1-5-21-7-8-9-1010").unwrap();
    assert!(machine.add_to_group(&user, &stranger).is_err());

    // A new password, and a weak one refused without being named.
    let renewed = machine.new_password();
    machine.set_password(&user, &renewed).unwrap();
    let weak = Password::new("hunter2hunter2".to_owned());
    let refused = machine.set_password(&user, &weak).unwrap_err();
    assert!(!refused.to_string().contains("hunter2"), "{refused}");

    // Hidden from the sign-in screen.
    machine.hide(&user).unwrap();
    assert_eq!(accounts::hidden(&user), Ok(true));

    // The record, whole, with its protected DACL; the owner is never a pool user.
    record.removing = true;
    machine.write_record(&record).unwrap();
    assert_eq!(machine.read_record(), Ok(Some(record.clone())));
    let sddl = key_sddl(TEST_KEY).unwrap();
    assert!(sddl.starts_with("O:BAD:P"), "{sddl}");
    for entry in ["(A;CI;KA;;;SY)", "(A;CI;KA;;;BA)", "(A;CI;KR;;;BU)"] {
        assert!(sddl.contains(entry), "{sddl} lacks {entry} ({RECORD_SDDL})");
    }
    assert_eq!(sddl.matches("(A;").count(), 3, "{sddl}");
    let mut bad = record.clone();
    bad.pool.push(owner);
    assert!(machine.write_record(&bad).is_err());
    assert_eq!(machine.read_record(), Ok(Some(record.clone())));

    // Everything off again; each delete a second time is a no-op.
    machine.delete_user(&user).unwrap();
    assert_eq!(accounts::hidden(&user), Ok(false));
    assert_eq!(machine.account_sid(&user), Ok(None));
    machine.delete_user(&user).unwrap();
    assert_eq!(machine.group_members(&group), Ok(0));
    machine.delete_group(&group).unwrap();
    assert_eq!(machine.group(), Ok(None));
    machine.delete_record().unwrap();
    assert_eq!(machine.read_record(), Ok(None));
    machine.delete_record().unwrap();
    machine.unlock();
}
