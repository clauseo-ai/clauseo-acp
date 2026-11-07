//! The refresh's order and refusals over a fake machine whose files are byte vectors: the
//! replace sequence, a crash at every step, the self-repair of a record a crash left stale, and
//! each refusal.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::test_signer::{TestKey, comment};
use super::*;

const OWN: &str = "1.4.0";
const ADMIN: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const INSTALL: &str = r"C:\Users\Rohan\AppData\Local\Clauseo";

/// The steps a crash can stop the run at, in the order they run.
const STEPS: [&str; 6] = [
    "stage",
    "secure_staged",
    "set_aside",
    "place_staged",
    "delete_later",
    "write_sha256",
];

/// The fake machine: each file as bytes, or `None` when absent.
struct Fake {
    system: bool,
    exe: PathBuf,
    record: RefreshRecord,
    source_exe: Result<Vec<u8>, SourceError>,
    source_sig: Result<Vec<u8>, SourceError>,
    /// What `stage` reads back, when it differs from what it was given.
    staged_reads_back: Option<Vec<u8>>,
    admin_file: Option<Vec<u8>>,
    staged: Option<Vec<u8>>,
    set_aside: Option<Vec<u8>>,
    /// The step that fails, as a failed call.
    fail: Option<&'static str>,
    /// The step the machine stops at: it and everything after do nothing.
    crash: Option<&'static str>,
    crashed: bool,
    calls: Vec<&'static str>,
    logs: Vec<(bool, String)>,
}

impl Fake {
    /// A machine where the admin copy is `old`, recorded, and the user folder holds `new`
    /// signed by `key` with `sig_comment`.
    fn new(old: &[u8], new: &[u8], key: &TestKey, sig_comment: &str) -> Fake {
        Fake {
            system: true,
            exe: PathBuf::from(ADMIN),
            record: RefreshRecord {
                user_install_dir: Some(INSTALL.to_owned()),
                admin_copy_sha256: Some(sha_hex(old)),
            },
            source_exe: Ok(new.to_vec()),
            source_sig: Ok(key.sign(new, sig_comment)),
            staged_reads_back: None,
            admin_file: Some(old.to_vec()),
            staged: None,
            set_aside: None,
            fail: None,
            crash: None,
            crashed: false,
            calls: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// Records the call; `Err` when it fails or the machine has crashed (doing nothing).
    fn step(&mut self, name: &'static str) -> Result<(), Failed> {
        self.calls.push(name);
        if self.crash == Some(name) {
            self.crashed = true;
        }
        if self.crashed {
            return Err(format!("{name}: the machine is down"));
        }
        if self.fail == Some(name) {
            return Err(format!("{name} failed"));
        }
        Ok(())
    }

    /// The invariant: the record never names a hash the admin path doesn't hold, unless it is
    /// the hash it held before the run (the old copy's, which a crash between the renames
    /// leaves with no file).
    fn record_names_what_is_there(&self, old: &[u8]) -> bool {
        match (&self.record.admin_copy_sha256, &self.admin_file) {
            (Some(hash), Some(file)) => *hash == sha_hex(file) || *hash == sha_hex(old),
            (Some(hash), None) => *hash == sha_hex(old),
            (None, _) => true,
        }
    }
}

impl Refresher for Fake {
    fn is_system(&mut self) -> Result<bool, Failed> {
        self.calls.push("is_system");
        Ok(self.system)
    }
    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        Ok(self.exe.clone())
    }
    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        Ok(PathBuf::from(ADMIN))
    }
    fn record(&mut self) -> Result<RefreshRecord, Failed> {
        self.calls.push("record");
        Ok(self.record.clone())
    }
    fn clear_leftovers(&mut self) -> Result<(), Failed> {
        self.step("clear_leftovers")?;
        self.set_aside = None;
        self.staged = None;
        Ok(())
    }
    fn read_source(&mut self, dir: &Path, which: SourceFile) -> Result<Vec<u8>, SourceError> {
        assert_eq!(dir, Path::new(INSTALL), "the record's folder, nothing else");
        self.calls.push("read_source");
        match which {
            SourceFile::Exe => self.source_exe.clone(),
            SourceFile::Sig => self.source_sig.clone(),
        }
    }
    fn stage(&mut self, exe: &[u8]) -> Result<Vec<u8>, Failed> {
        self.step("stage")?;
        let staged = self
            .staged_reads_back
            .clone()
            .unwrap_or_else(|| exe.to_vec());
        self.staged = Some(staged.clone());
        Ok(staged)
    }
    fn admin_copy_sha256(&mut self) -> Result<Option<[u8; 32]>, Failed> {
        Ok(self
            .admin_file
            .as_ref()
            .map(|file| Sha256::digest(file).into()))
    }
    fn secure_staged(&mut self) -> Result<(), Failed> {
        self.step("secure_staged")
    }
    fn set_aside_admin_copy(&mut self) -> Result<(), Failed> {
        self.step("set_aside")?;
        self.set_aside = self.admin_file.take();
        Ok(())
    }
    fn place_staged(&mut self) -> Result<(), Failed> {
        self.step("place_staged")?;
        assert!(self.admin_file.is_none(), "a move never replaces a file");
        self.admin_file = self.staged.take();
        Ok(())
    }
    fn restore_admin_copy(&mut self) -> Result<(), Failed> {
        self.step("restore")?;
        self.admin_file = self.set_aside.take();
        Ok(())
    }
    fn delete_set_aside_later(&mut self) -> Result<(), Failed> {
        self.step("delete_later")
    }
    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed> {
        self.step("write_sha256")?;
        self.record.admin_copy_sha256 = Some(hex.to_owned());
        Ok(())
    }
    fn discard_staged(&mut self) {
        self.calls.push("discard_staged");
        if !self.crashed {
            self.staged = None;
        }
    }
    fn log(&mut self, error: bool, line: &str) {
        self.logs.push((error, line.to_owned()));
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn old() -> Vec<u8> {
    b"MZ the old setup exe".to_vec()
}

fn new() -> Vec<u8> {
    b"MZ the new setup exe, signed".to_vec()
}

fn key() -> TestKey {
    TestKey::new(0x5EED_0000_0000_0001)
}

fn good() -> String {
    comment(signature::SIGNED_FILE, OWN)
}

#[test]
fn a_signed_newer_exe_replaces_the_admin_copy_then_the_record() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(refresh(&mut fake, &key.public(), OWN), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(new()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&new())));
    assert_eq!(
        fake.set_aside,
        Some(old()),
        "kept until the reboot or the next run"
    );
    let order: Vec<_> = fake
        .calls
        .iter()
        .filter(|call| STEPS.contains(call))
        .collect();
    assert_eq!(order, STEPS.iter().collect::<Vec<_>>());
    assert_eq!(
        fake.logs,
        [(false, "the sandbox's admin copy was refreshed".to_owned())]
    );
}

#[test]
fn a_crash_at_any_step_never_leaves_a_hash_for_a_file_that_isnt_there() {
    let key = key();
    for crash in STEPS {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.crash = Some(crash);
        let _exit = refresh(&mut fake, &key.public(), OWN);
        assert!(fake.record_names_what_is_there(&old()), "crash at {crash}");
        assert_ne!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&new())),
            "crash at {crash}: the new hash is written last"
        );
        // The machine comes back and the task runs again (from whichever copy is in place):
        // it ends with the new copy, recorded.
        if fake.admin_file.is_none() {
            continue; // Only a re-adopt helps: the task's exe is gone (the launcher asks).
        }
        fake.crash = None;
        fake.crashed = false;
        assert_eq!(
            refresh(&mut fake, &key.public(), OWN),
            RefreshExit::Done,
            "after {crash}"
        );
        assert_eq!(fake.admin_file, Some(new()), "after {crash}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&new())),
            "after {crash}"
        );
    }
}

#[test]
fn a_new_copy_left_unrecorded_gets_its_hash_without_another_swap() {
    // A crash after the move left the new copy under the old hash.
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.admin_file = Some(new());
    assert_eq!(refresh(&mut fake, &key.public(), OWN), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&new())));
    assert!(!fake.calls.contains(&"set_aside"));
    assert_eq!(fake.staged, None, "the staged copy is discarded");
}

#[test]
fn an_exe_already_recorded_changes_nothing() {
    let key = key();
    let mut fake = Fake::new(&new(), &new(), &key, &good());
    assert_eq!(refresh(&mut fake, &key.public(), OWN), RefreshExit::Done);
    assert!(!fake.calls.contains(&"set_aside") && !fake.calls.contains(&"write_sha256"));
    assert_eq!(fake.staged, None);
    assert_eq!(
        fake.logs,
        [(false, "the sandbox's admin copy is current".to_owned())]
    );
}

#[test]
fn a_failed_move_puts_the_old_copy_back() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("place_staged");
    assert_eq!(
        refresh(&mut fake, &key.public(), OWN),
        RefreshExit::ReplaceFailed
    );
    assert_eq!(fake.admin_file, Some(old()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&old())));
    assert_eq!(fake.staged, None, "the staged copy is discarded");
    let (error, line) = fake.logs.last().unwrap();
    assert!(
        *error && line.contains("the old copy is back in place"),
        "{line}"
    );
}

#[test]
fn a_failed_set_aside_or_securing_keeps_the_old_copy() {
    let key = key();
    for (fail, exit) in [
        ("set_aside", RefreshExit::ReplaceFailed),
        ("secure_staged", RefreshExit::StagingFailed),
        ("stage", RefreshExit::StagingFailed),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.fail = Some(fail);
        assert_eq!(refresh(&mut fake, &key.public(), OWN), exit, "{fail}");
        assert_eq!(fake.admin_file, Some(old()), "{fail}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{fail}"
        );
        assert!(!fake.calls.contains(&"place_staged"), "{fail}");
    }
}

#[test]
fn a_failed_record_write_exits_its_own_code() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("write_sha256");
    assert_eq!(
        refresh(&mut fake, &key.public(), OWN),
        RefreshExit::RecordNotWritten
    );
    assert_eq!(fake.admin_file, Some(new()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&old())));
}

#[test]
fn only_system_from_the_admin_copy_runs_it() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.system = false;
    assert_eq!(
        refresh(&mut fake, &key.public(), OWN),
        RefreshExit::NotAllowed
    );
    assert_eq!(fake.calls, ["is_system"], "nothing read");
    for elsewhere in [
        r"C:\Users\Rohan\AppData\Local\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe.new",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(elsewhere);
        assert_eq!(
            refresh(&mut fake, &key.public(), OWN),
            RefreshExit::NotAllowed
        );
        assert!(!fake.calls.contains(&"record"), "{elsewhere}");
    }
    // The same path in another case or the verbatim form is the admin copy.
    for same in [ADMIN.to_uppercase(), format!(r"\\?\{ADMIN}")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.exe = PathBuf::from(same);
        assert_eq!(refresh(&mut fake, &key.public(), OWN), RefreshExit::Done);
    }
}

#[test]
fn the_placeholder_key_refuses_before_reading_anything() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(
        refresh(&mut fake, key::PLACEHOLDER_KEY, OWN),
        RefreshExit::PlaceholderKey
    );
    assert!(!fake.calls.contains(&"record") && !fake.calls.contains(&"read_source"));
    assert!(fake.logs.iter().all(|(error, _)| *error));
}

#[test]
fn a_missing_or_odd_install_folder_is_no_record() {
    let key = key();
    for dir in [
        None,
        Some(r"relative\Clauseo"),
        Some(r"C:\Users\Rohan\..\Admin"),
        Some(r"\\server\share\Clauseo"),
        Some(r"\\?\C:\Users\Rohan"),
        Some(r"C:\Users\Rohan\AppData:stream"),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.record.user_install_dir = dir.map(str::to_owned);
        assert_eq!(
            refresh(&mut fake, &key.public(), OWN),
            RefreshExit::NoRecord,
            "{dir:?}"
        );
        assert!(!fake.calls.contains(&"read_source"), "{dir:?}");
    }
}

#[test]
fn a_refused_source_file_stages_nothing() {
    let key = key();
    for error in [
        SourceError::Missing,
        SourceError::ReparsePoint(0xA000_000C),
        SourceError::NotRegular,
        SourceError::Linked(2),
        SourceError::TooLarge(EXE_CAP),
    ] {
        for exe_side in [true, false] {
            let mut fake = Fake::new(&old(), &new(), &key, &good());
            if exe_side {
                fake.source_exe = Err(error.clone());
            } else {
                fake.source_sig = Err(error.clone());
            }
            assert_eq!(
                refresh(&mut fake, &key.public(), OWN),
                RefreshExit::SourceRefused
            );
            assert!(!fake.calls.contains(&"stage"), "{error:?}");
            assert_eq!(fake.admin_file, Some(old()));
            // The line names the file, the install folder and why; no other path.
            let (_, line) = fake.logs.last().unwrap();
            assert!(
                line.contains(INSTALL) && line.contains(&error.to_string()),
                "{line}"
            );
        }
    }
}

/// A change to the fake machine before a run.
type SetUp<'a> = Box<dyn Fn(&mut Fake) + 'a>;

#[test]
fn a_bad_signature_keeps_the_old_copy() {
    let key = key();
    let other = TestKey::new(0x0BAD_0000_0000_0002);
    let cases: [(&str, SetUp<'_>); 5] = [
        (
            "wrong key",
            Box::new(|fake| fake.source_sig = Ok(other.sign(&new(), &good()))),
        ),
        (
            "garbage",
            Box::new(|fake| fake.source_sig = Ok(b"garbage".to_vec())),
        ),
        (
            "other file",
            Box::new(|fake| {
                fake.source_sig = Ok(key.sign(&new(), &comment("Clauseo-setup.exe", OWN)));
            }),
        ),
        (
            "downgrade",
            Box::new(|fake| {
                fake.source_sig = Ok(key.sign(&new(), &comment(signature::SIGNED_FILE, "1.3.0")));
            }),
        ),
        // The user swaps the exe after the read: what staging reads back is what is checked.
        (
            "swapped",
            Box::new(|fake| fake.staged_reads_back = Some(b"MZ evil".to_vec())),
        ),
    ];
    for (name, set_up) in cases {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        set_up(&mut fake);
        assert_eq!(
            refresh(&mut fake, &key.public(), OWN),
            RefreshExit::SignatureRefused,
            "{name}"
        );
        assert_eq!(fake.admin_file, Some(old()), "{name}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{name}"
        );
        assert_eq!(fake.staged, None, "{name}: the staged copy is discarded");
        assert!(!fake.calls.contains(&"set_aside"), "{name}");
    }
}

#[test]
fn exit_codes_are_distinct_and_clear_of_the_set_up_codes() {
    let all = [
        RefreshExit::Done,
        RefreshExit::NotAllowed,
        RefreshExit::NoRecord,
        RefreshExit::SourceRefused,
        RefreshExit::SignatureRefused,
        RefreshExit::PlaceholderKey,
        RefreshExit::StagingFailed,
        RefreshExit::ReplaceFailed,
        RefreshExit::RecordNotWritten,
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 32, 33, 34, 35, 36, 37, 38, 39]);
    let setup_max = crate::windows::setup::SetupExit::ALL
        .iter()
        .map(|exit| exit.as_code())
        .max()
        .unwrap();
    assert!(setup_max < 32);
}

#[test]
fn the_descriptors_say_what_the_module_says() {
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_eq!(
        task_sddl(&owner),
        "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    // Read back with the flags SetSecurityInfo adds, the staging DACL still matches; any other
    // entry, owner or a lost protection doesn't.
    assert!(sddl_matches("O:SYD:PAI(A;OICI;FA;;;SY)", STAGING_SDDL));
    assert!(sddl_matches("O:SYD:P(A;OICI;FA;;;SY)", STAGING_SDDL));
    for wrong in [
        "O:SYD:AI(A;OICI;FA;;;SY)",
        "O:BAD:PAI(A;OICI;FA;;;SY)",
        "O:SYD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BU)",
        "O:SYD:PAI(A;OICI;FA;;;BA)",
        "O:SY",
    ] {
        assert!(!sddl_matches(wrong, STAGING_SDDL), "{wrong}");
    }
    assert_eq!(REFRESH_ARG, "--refresh");
    assert_eq!(EXECUTION_LIMIT, "PT5M");
}
