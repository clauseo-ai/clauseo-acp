//! Links in a real pool user's profile (ADR 0049, "Reparse points"): the `LocalLow` hand-off
//! to `Administrators` is refused, changing nothing, when a part on the way is a junction,
//! whether the profile is unloaded or loaded by a process running as the user. The links the
//! user plants itself, and a profile shut to `Administrators`, are in `links_by_user.rs`.
//!
//! `elevated_` and `#[ignore]`: they run only from CI's elevated step, behind [`gate`]. Their
//! accounts are `ClauseoSbTK` and six hex digits ([`LINKS_TEST_PREFIX`]), so their sweep never
//! clears another family's. The real-NTFS tests without an account are in
//! `acl/profile/tests.rs`.
#![expect(
    unsafe_code,
    reason = "FFI to read whether the test user's hive is loaded"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY, HKEY_USERS, KEY_READ, RegOpenKeyExW};
use windows::core::HSTRING;

use super::super::super::accounts_tests::machine_on;
use super::super::super::machine::{Machine, MachineError};
use super::super::super::profile::{delete_profile, profile_of};
use super::super::super::record_write::Key;
use super::super::super::windows_machine::Names;
use super::super::log_on_once;
use super::as_user::Running;
use super::{TestUser, clear_leftovers, gate, me, random_hex};
use crate::test_support::{TempDir, try_rename};
use crate::windows::acl::object::Object;
use crate::windows::acl::profile::{Below, open_below, set_owner_below};
use crate::windows::acl::{descriptor, owner};
use crate::windows::setup::strays::LINKS_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::{give_owner, protect_with};
use crate::windows::test_junction::junction;

/// A canary's DACL: protected, so nothing it inherits changes under it.
const CANARY_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)";
/// What a canary's folder lets its reader do, on the folder alone: list and pass through
/// (`FILE_GENERIC_READ | FILE_TRAVERSE`).
const READER_FOLDER: &str = "0x1200a9";
/// What a canary's file lets its reader do: read, and write its attributes
/// (`FILE_GENERIC_READ | FILE_WRITE_ATTRIBUTES`), which a new name for the file needs.
const READER_FILE: &str = "0x120189";
/// `BUILTIN\Administrators`, whom the set-up hands `LocalLow` to.
pub(super) const ADMINISTRATORS: &str = "S-1-5-32-544";
/// How many times a step waiting on the profile service is tried.
const TRIES: u32 = 60;
/// The wait between two tries.
const WAIT: Duration = Duration::from_millis(500);

/// A folder outside the profile, with what it held and its descriptors (owner and DACL).
pub(super) struct Canary {
    pub(super) folder: PathBuf,
    pub(super) file: PathBuf,
    before: Vec<(PathBuf, Vec<u8>)>,
}

impl Canary {
    /// `canary\keep.txt` and `canary\LocalLow` in `dir`, each with [`CANARY_SDDL`], and
    /// `LocalLow` owned by this process's user, so a hand-off to `Administrators` would show.
    /// A `reader` may also list the folder and give the file a new name ([`READER_FOLDER`],
    /// [`READER_FILE`]), as a test user planting a hard link to it must.
    pub(super) fn new(dir: &TempDir, reader: Option<&Sid>) -> Canary {
        let file = dir.file("canary/keep.txt", "kept");
        let local_low = dir.dir("canary/LocalLow");
        let folder = dir.path().join("canary");
        let with = |rights: &str| match reader {
            Some(sid) => format!("{CANARY_SDDL}(A;;{rights};;;{})", sid.as_str()),
            None => CANARY_SDDL.to_owned(),
        };
        protect_with(&folder, &with(READER_FOLDER)).unwrap();
        protect_with(&file, &with(READER_FILE)).unwrap();
        protect_with(&local_low, CANARY_SDDL).unwrap();
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
    pub(super) fn assert_unchanged(&self) {
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
pub(super) fn user_with_profile() -> (TestUser, PathBuf) {
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
    assert_eq!(machine.owner_in_profile(profile, &local_low), Err(changed));
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

/// Waits until `sid`'s hive is `loaded` (or not), for up to [`TRIES`] waits.
pub(super) fn wait_for_hive(sid: &Sid, loaded: bool, running: Option<&Running>) {
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
fn elevated_a_junction_at_app_data_stops_the_local_low_hand_off_with_its_profile_unloaded() {
    if !gate(
        "elevated_a_junction_at_app_data_stops_the_local_low_hand_off_with_its_profile_unloaded",
    ) {
        return;
    }
    let dir = TempDir::new("links-unloaded");
    let canary = Canary::new(&dir, None);
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
    let canary = Canary::new(&dir, None);
    let (user, profile) = user_with_profile();
    let running = Running::ping(&user);
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
