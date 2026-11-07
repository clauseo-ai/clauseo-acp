//! Links in a real pool user's profile (ADR 0049, "Reparse points"): the profile delete takes
//! out every junction, symbolic link and extra name of a file first, so what they name keeps
//! its content, owner and DACL; and the `LocalLow` hand-off to `Administrators` is refused,
//! changing nothing, when a part on the way is a junction, whether the profile is unloaded or
//! loaded by a process running as the user.
//!
//! `elevated_` and `#[ignore]`: they run only from CI's elevated step, behind [`gate`]. Their
//! accounts are `ClauseoSbTK` and six hex digits ([`LINKS_TEST_PREFIX`]), so their sweep never
//! clears another family's. The real-NTFS tests without an account are in
//! `acl/profile/tests.rs`.
#![expect(
    unsafe_code,
    reason = "FFI to start a process as the test user and to read its hive's load"
)]

use std::iter::once;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{HKEY, HKEY_USERS, KEY_READ, RegOpenKeyExW};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessWithLogonW, LOGON_WITH_PROFILE, PROCESS_INFORMATION,
    STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};
use zeroize::Zeroizing;

use super::super::super::accounts_tests::machine_on;
use super::super::super::machine::{Machine, MachineError};
use super::super::super::profile::{delete_profile, profile_of};
use super::super::super::record_write::Key;
use super::super::super::windows_machine::Names;
use super::super::log_on_once;
use super::{TestUser, clear_leftovers, gate, me, random_hex};
use crate::test_support::{TempDir, hard_link_at, symlink_dir_at, symlink_file_at, try_rename};
use crate::windows::acl::object::Object;
use crate::windows::acl::profile::{Below, open_below, set_owner_below};
use crate::windows::acl::{descriptor, owner};
use crate::windows::setup::strays::LINKS_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::{give_owner, protect_with};
use crate::windows::test_junction::junction;

/// A canary's DACL: protected, so nothing it inherits changes under it.
const CANARY_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)";
/// `BUILTIN\Administrators`, whom the set-up hands `LocalLow` to.
const ADMINISTRATORS: &str = "S-1-5-32-544";
/// How many times a step waiting on the profile service is tried.
const TRIES: u32 = 60;
/// The wait between two tries.
const WAIT: Duration = Duration::from_millis(500);

/// A folder outside the profile, with what it held and its descriptors (owner and DACL).
struct Canary {
    folder: PathBuf,
    file: PathBuf,
    before: Vec<(PathBuf, Vec<u8>)>,
}

impl Canary {
    /// `canary\keep.txt` and `canary\LocalLow` in `dir`, each with [`CANARY_SDDL`], and
    /// `LocalLow` owned by this process's user, so a hand-off to `Administrators` would show.
    fn new(dir: &TempDir) -> Canary {
        let file = dir.file("canary/keep.txt", "kept");
        let local_low = dir.dir("canary/LocalLow");
        let folder = dir.path().join("canary");
        for path in [&folder, &file, &local_low] {
            protect_with(path, CANARY_SDDL).unwrap();
        }
        give_owner(&local_low, &me()).unwrap();
        let before = [&folder, &file, &local_low]
            .into_iter()
            .map(|path| (path.clone(), descriptor(path).unwrap()))
            .collect();
        Canary {
            folder,
            file,
            before,
        }
    }

    /// Asserts each part is there with its content, owner and DACL as they were, and the file
    /// has one name.
    fn assert_unchanged(&self) {
        assert!(self.folder.is_dir(), "{}", self.folder.display());
        assert_eq!(std::fs::read_to_string(&self.file).unwrap(), "kept");
        for (path, before) in &self.before {
            assert_eq!(&descriptor(path).unwrap(), before, "{}", path.display());
        }
        assert_eq!(Object::open(&self.file).unwrap().links, 1);
    }
}

/// A test user of this family, with its profile made by one logon; the profile's folder as
/// `ProfileList` lists it, which the set-up reaches it by.
fn user_with_profile() -> (TestUser, PathBuf) {
    clear_leftovers(LINKS_TEST_PREFIX);
    let user = TestUser::named(LINKS_TEST_PREFIX, &random_hex(3));
    let made = log_on_once(&user.name, &user.password).unwrap();
    assert!(!made.temporary, "{made:?}");
    let listed = profile_of(&user.sid).unwrap().unwrap();
    (user, listed.path)
}

/// `Administrators`.
fn admins() -> Sid {
    Sid::parse(ADMINISTRATORS).unwrap()
}

/// Waits [`WAIT`] before a step on the profile service is tried again.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait for the profile service to load or unload a hive, with no clock"
)]
fn wait() {
    std::thread::sleep(WAIT);
}

/// Moves `profile\AppData` to `AppData.real`, tried again while the profile service still
/// holds the hive in it after a logon.
fn move_app_data(profile: &Path) {
    let (from, to) = (profile.join("AppData"), profile.join("AppData.real"));
    for tries in 1.. {
        match try_rename(&from, &to) {
            Ok(()) => return,
            Err(error) if tries < TRIES => eprintln!("AppData not moved yet: {error}"),
            Err(error) => panic!("AppData wasn't moved: {error}"),
        }
        wait();
    }
}

/// Asserts the hand-off of `profile`'s `AppData\LocalLow` stops at `AppData`, by the call and
/// through the set-up's own machine, and the canary is unchanged.
fn assert_refused(profile: &Path, canary: &Canary) {
    let local_low = profile.join("AppData").join("LocalLow");
    let refused = set_owner_below(profile, &local_low, &admins()).unwrap_err();
    assert!(
        matches!(&refused, Below::Changed(part) if part == "AppData"),
        "{refused:?}"
    );
    let mut machine = machine_on(Names::production(), Path::new(r"C:\ClauseoNoSuchData"));
    let changed = MachineError::ProfileChanged("AppData".to_owned());
    assert_eq!(
        machine.give_to_admins(profile, &local_low),
        Err(changed.clone())
    );
    assert_eq!(machine.in_profile(profile, &local_low), Err(changed));
    canary.assert_unchanged();
}

/// Asserts the hand-off of `profile`'s own `AppData\LocalLow` (`user`'s) reaches it: handed to
/// `Administrators`, or, if refused, left with an owner the profile service gives.
fn assert_hand_off_reaches(profile: &Path, user: &Sid) {
    let local_low = profile.join("AppData").join("LocalLow");
    assert!(open_below(profile, &local_low).is_ok());
    let Err(refused) = set_owner_below(profile, &local_low, &admins()) else {
        assert_eq!(owner(&local_low).unwrap(), admins());
        return;
    };
    let now = owner(&local_low).unwrap();
    eprintln!(
        "the hand-off was refused ({refused:?}); owner {}",
        now.as_str()
    );
    let system = Sid::parse("S-1-5-18").unwrap();
    assert!(
        [system, user.clone(), me(), admins()].contains(&now),
        "{}",
        now.as_str()
    );
}

/// Whether `sid`'s hive is loaded: `HKU\<sid>` is there.
fn hive_loaded(sid: &Sid) -> bool {
    let path = HSTRING::from(sid.as_str());
    let mut opened = HKEY::default();
    // SAFETY: the path is NUL-terminated and lives for the call; `opened` is a live out-pointer,
    // closed by `Key` once found.
    let found = unsafe { RegOpenKeyExW(HKEY_USERS, &path, None, KEY_READ, &raw mut opened) };
    if found == ERROR_SUCCESS {
        drop(Key(opened));
        return true;
    }
    false
}

/// A process this test started as the test user; ended (its handle alone) and waited for on
/// drop, so a failed assertion still stops it.
struct Running(PROCESS_INFORMATION);

impl Running {
    /// `ping` for five minutes as `user`, its profile loaded (`LOGON_WITH_PROFILE`), with no
    /// window, through the Secondary Logon service. The password goes in a buffer wiped on drop.
    fn start(user: &TestUser) -> Running {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        let folder = HSTRING::from(format!(r"{system}\System32"));
        let ping = HSTRING::from(format!(r"{system}\System32\PING.EXE"));
        let mut command: Vec<u16> = format!(r#""{ping}" -n 300 127.0.0.1"#)
            .encode_utf16()
            .chain(once(0))
            .collect();
        let password: Zeroizing<Vec<u16>> =
            Zeroizing::new(user.password.encode_utf16().chain(once(0)).collect());
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap(),
            ..STARTUPINFOW::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every string is NUL-terminated and lives for the call; the command line is a
        // mutable buffer, as the call needs; `startup` and `info` are live.
        let started = unsafe {
            CreateProcessWithLogonW(
                &HSTRING::from(user.name.as_str()),
                w!("."),
                PCWSTR(password.as_ptr()),
                LOGON_WITH_PROFILE,
                &ping,
                Some(PWSTR(command.as_mut_ptr())),
                CREATE_NO_WINDOW,
                None,
                &folder,
                &raw const startup,
                &raw mut info,
            )
        };
        if let Err(error) = started {
            panic!("CreateProcessWithLogonW (needs the Secondary Logon service): {error}");
        }
        Running(info)
    }

    /// Whether it has ended.
    fn ended(&self) -> bool {
        // SAFETY: the handle came from CreateProcessWithLogonW and is open until drop.
        let waited = unsafe { WaitForSingleObject(self.0.hProcess, 0) };
        waited == WAIT_OBJECT_0
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // SAFETY: both handles came from CreateProcessWithLogonW and are closed once, here;
        // only this process is ended.
        unsafe {
            if let Err(error) = TerminateProcess(self.0.hProcess, 1) {
                eprintln!("the test's process wasn't ended: {error}");
            }
            WaitForSingleObject(self.0.hProcess, 10_000);
            drop(CloseHandle(self.0.hThread));
            drop(CloseHandle(self.0.hProcess));
        }
    }
}

/// Waits until `sid`'s hive is `loaded` (or not), for up to [`TRIES`] waits.
fn wait_for_hive(sid: &Sid, loaded: bool, running: Option<&Running>) {
    for _ in 0..TRIES {
        if hive_loaded(sid) == loaded {
            return;
        }
        assert!(
            !running.is_some_and(Running::ended),
            "the test user's process ended before its profile was loaded"
        );
        wait();
    }
    panic!("the test user's hive didn't become loaded: {loaded}, in {TRIES} waits");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_profile_delete_leaves_what_its_links_name_as_it_was() {
    if !gate("elevated_a_profile_delete_leaves_what_its_links_name_as_it_was") {
        return;
    }
    let dir = TempDir::new("links-profile-delete");
    let canary = Canary::new(&dir);
    let (user, profile) = user_with_profile();
    junction(&profile.join("Junction"), &canary.folder).unwrap();
    let local = profile.join("AppData").join("Local");
    symlink_dir_at(&local.join("DirLink"), &canary.folder);
    symlink_file_at(&profile.join("FileLink.txt"), &canary.file);
    let linked = local.join("Linked.txt");
    match hard_link_at(&linked, &canary.file) {
        Ok(()) => assert_eq!(Object::open(&canary.file).unwrap().links, 2),
        // ERROR_NOT_SAME_DEVICE: the temp folder is on another volume than the profile.
        Err(error) if error.raw_os_error() == Some(17) => {
            eprintln!("skipped the hard link: {error}");
        }
        Err(error) => panic!("hard link: {error}"),
    }

    delete_profile(&user.sid, &me()).unwrap();
    assert!(!profile.exists(), "{}", profile.display());
    assert_eq!(profile_of(&user.sid).unwrap(), None);
    canary.assert_unchanged();
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_junction_at_app_data_stops_the_local_low_hand_off_with_its_profile_unloaded() {
    if !gate(
        "elevated_a_junction_at_app_data_stops_the_local_low_hand_off_with_its_profile_unloaded",
    ) {
        return;
    }
    let dir = TempDir::new("links-unloaded");
    let canary = Canary::new(&dir);
    let (user, profile) = user_with_profile();
    move_app_data(&profile);
    junction(&profile.join("AppData"), &canary.folder).unwrap();

    assert_refused(&profile, &canary);
    delete_profile(&user.sid, &me()).unwrap();
    assert!(!profile.exists(), "{}", profile.display());
    canary.assert_unchanged();
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_local_low_hand_off_cant_be_steered_while_the_profile_is_loaded() {
    if !gate("elevated_the_local_low_hand_off_cant_be_steered_while_the_profile_is_loaded") {
        return;
    }
    let dir = TempDir::new("links-loaded");
    let canary = Canary::new(&dir);
    let (user, profile) = user_with_profile();
    let running = Running::start(&user);
    wait_for_hive(&user.sid, true, Some(&running));

    let moved = try_rename(&profile.join("AppData"), &profile.join("AppData.real"));
    if let Err(error) = moved {
        eprintln!("loaded: AppData stays put ({error}); the hand-off reaches the real one");
        assert_hand_off_reaches(&profile, &user.sid);
    } else {
        eprintln!("loaded: AppData moved; a junction in its place is refused");
        junction(&profile.join("AppData"), &canary.folder).unwrap();
        assert_refused(&profile, &canary);
    }

    drop(running);
    wait_for_hive(&user.sid, false, None);
    delete_profile(&user.sid, &me()).unwrap();
    assert!(!profile.exists(), "{}", profile.display());
    canary.assert_unchanged();
}
