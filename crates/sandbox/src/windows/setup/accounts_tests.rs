//! `WindowsMachine`'s account, group, hidden-user and record methods on a real Windows. The
//! unelevated tests run in CI's ordinary step; the `elevated_` ones only in its elevated step
//! (`#[ignore]`, and skipped at run time unless the token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`, D-B16). They use `ClauseoSbTest` names with a random suffix and
//! the key `HKLM\SOFTWARE\Clauseo\SandboxTest`, never the real pool or record, and each first
//! clears what a killed earlier try left (split §8): its record names everything it made.

use std::io;
use std::path::{Path, PathBuf};

use super::accounts::{self, Kind};
use super::lock_tests::{suffix, unique_mutex};
use super::machine::{Machine, MachineError, Password};
use super::password::is_strong;
use super::record_values::RECORD_SDDL;
use super::record_write::{self, key_sddl};
use super::windows_machine::{Names, WindowsMachine};
use crate::windows::FolderSupport;
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::pool::MachineRecord;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::elevated_tests_enabled;

/// The tests' record key.
const TEST_KEY: &str = "SOFTWARE\\Clauseo\\SandboxTest";

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

/// Deletes every account and group the test record names, then the record: what a killed
/// earlier try left, or this try's own at the end. Each step goes on past a failure.
fn sweep() {
    let Ok(Some(record)) = record_write::read(TEST_KEY) else {
        drop(record_write::delete(TEST_KEY));
        return;
    };
    for sid in &record.pool {
        if let Ok(Some(name)) = accounts::name_of(sid, Kind::User) {
            drop(accounts::delete_user(&name));
        }
    }
    if let Ok(Some(name)) = accounts::name_of(&record.group, Kind::Group) {
        drop(accounts::delete_group(&name));
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
    let mut real = WindowsMachine::new(Names::production(), Box::new(NoSurroundings));
    assert!(real.read_record().is_ok());
    assert_eq!(crate::windows::sandbox_pool_sids().ok(), Some(vec![]));
}

#[test]
fn lookups_answer_for_missing_and_non_local_accounts() {
    let mut machine = WindowsMachine::new(Names::production(), Box::new(NoSurroundings));
    let nobody = format!("ClauseoSbTest{}", suffix());
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

#[test]
fn the_machine_answers_unsupported_for_what_later_prs_build() {
    let mut machine = WindowsMachine::new(Names::production(), Box::new(NoSurroundings));
    assert_eq!(
        machine.seclogon(),
        Err(MachineError::Unsupported("seclogon"))
    );
    let sid = Sid::parse("S-1-5-21-7-8-9-1010").unwrap();
    assert_eq!(
        machine.delete_profile(&sid),
        Err(MachineError::Unsupported("delete_profile"))
    );
    assert_eq!(
        machine.protect_for_owner(&[], &[]),
        Err(MachineError::Unsupported("protect_for_owner"))
    );
    assert!(machine.now() > 1_700_000_000);
    assert!(is_strong(&machine.new_password()));
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
    let user = format!("ClauseoSbTest{tag}");
    assert!(user.len() <= 20, "{user}");
    let names = Names {
        record_key: TEST_KEY.to_owned(),
        group: format!("ClauseoSbTestG{tag}"),
        mutex: unique_mutex().unwrap(),
    };
    let mut machine = WindowsMachine::new(names, Box::new(NoSurroundings));
    machine.lock().unwrap();

    // The group, recorded before any account so a killed try's sweep finds everything.
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
        data_dir: PathBuf::from(r"C:\ClauseoSbTest\data"),
        removing: false,
    };
    machine.write_record(&record).unwrap();
    assert_eq!(machine.read_record(), Ok(Some(record.clone())));

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
    assert!(sddl.starts_with("D:P"), "{sddl}");
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
