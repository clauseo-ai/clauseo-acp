//! The cross-check's file-and-cache rule on a real machine (`unlisted.rs`, `file_trust.rs`): a
//! task the test's own standard user registers, whose state this elevated process then changes
//! as an administrator, never as that user: its cache key's `SD` value overwritten (ownerless,
//! `O:SY`, unparsable, over its cap), and its task file rewritten to name SYSTEM and re-locked
//! so the user can't write it. The judging function ([`not_pool_by_path`]) reads the real
//! registry and file and must keep the task 56 in each case. Gated like the tests above.

#![expect(
    unsafe_code,
    reason = "the registry calls that read and overwrite the test task's cache SD, and the \
              SDDL conversion"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{GetSecurityDescriptorLength, PSECURITY_DESCRIPTOR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY,
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::super::file_trust::{ODD_SD, not_pool_by_path};
use super::super::windows_sweeper::WindowsSweeper;
use super::super::{Names, Sweeper as _, task_file, trusted};
use super::{Made, gate, system32, try_in_cmd};
use crate::test_support::TempDir;
use crate::windows::private_dir::set_owner;
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::protect_with;

/// The Task Scheduler's cache of task paths, below `HKEY_LOCAL_MACHINE` (`windows_listing.rs`).
const TREE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree";

/// An ownerless descriptor, readable by SYSTEM and Administrators.
const OWNERLESS: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)";

/// The test user's own task in the root folder, registered as that user. Dropped field by
/// field in this order: the cache `SD` put back, the task deleted, its folder, then the user.
struct Fixture {
    _restore: Restore,
    _made: Made,
    _dir: TempDir,
    user: TestUser,
    name: String,
}

impl Fixture {
    /// A new test user and its task `ClauseoSweep<case><suffix>`, a daily trigger far ahead.
    fn new(case: &str) -> Fixture {
        clear_leftovers(TASK_TEST_PREFIX);
        let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
        eprintln!("1. the test user: {} {}", user.name, user.sid.as_str());
        let dir = TempDir::new("sweep-forged");
        let grant = format!(
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
            user.sid.as_str()
        );
        protect_with(dir.path(), &grant).unwrap();
        let name = format!("ClauseoSweep{case}{}{}", std::process::id(), random_hex(2));
        let made = Made(vec![name.clone()]);
        let schtasks = format!(r"{}\schtasks.exe", system32());
        let action = format!(r"{}\cmd.exe /c exit 0", system32());
        let create =
            format!(r#""{schtasks}" /create /tn {name} /tr "{action}" /sc once /st 23:59 /f"#);
        let (code, printed) = try_in_cmd(&user, dir.path(), &create);
        eprintln!("2. as the test user: {create}\nexit {code:?}\n{printed}");
        assert_eq!(
            code,
            Some(0),
            "schtasks /create as the test user:\n{printed}"
        );
        let restore = Restore {
            name: name.clone(),
            sd: CacheKey::open(&name).unwrap().sd(),
        };
        Fixture {
            _restore: restore,
            _made: made,
            _dir: dir,
            user,
            name,
        }
    }

    /// Overwrites the task's cache `SD` value with `bytes`, as this administrator.
    fn set_sd(&self, bytes: &[u8]) {
        CacheKey::open(&self.name).unwrap().set_sd(bytes).unwrap();
    }
}

/// The task's original cache `SD`, put back on drop so the clean-up's delete finds it as made.
struct Restore {
    name: String,
    sd: Vec<u8>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let put = CacheKey::open(&self.name).and_then(|key| key.set_sd(&self.sd));
        if let Err(code) = put {
            eprintln!("the clean-up couldn't put the cache SD back ({code})");
        }
    }
}

/// A task's key below [`TREE`], opened for reading and setting values in the 64-bit view.
struct CacheKey(HKEY);

impl CacheKey {
    /// The key of the root folder's task `name`; `Err` with the Win32 code.
    fn open(name: &str) -> Result<CacheKey, u32> {
        let path = HSTRING::from(format!(r"{TREE}\{name}"));
        let access = KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY;
        let mut key = HKEY::default();
        // SAFETY: `path` is NUL-terminated and outlives the call; `key` is a live out-pointer.
        let opened =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &path, None, access, &raw mut key) };
        if opened == ERROR_SUCCESS {
            Ok(CacheKey(key))
        } else {
            Err(opened.0)
        }
    }

    /// Its `SD` value's bytes.
    fn sd(&self) -> Vec<u8> {
        let mut length = 0_u32;
        // SAFETY: a live key; a size query, `length` a live out-pointer.
        let sized =
            unsafe { RegQueryValueExW(self.0, w!("SD"), None, None, None, Some(&raw mut length)) };
        assert_eq!(sized, ERROR_SUCCESS, "the SD value's size");
        let mut bytes = vec![0_u8; usize::try_from(length).unwrap()];
        // SAFETY: `bytes` is writable for `length` bytes; the out-pointers are live.
        let read = unsafe {
            RegQueryValueExW(
                self.0,
                w!("SD"),
                None,
                None,
                Some(bytes.as_mut_ptr()),
                Some(&raw mut length),
            )
        };
        assert_eq!(read, ERROR_SUCCESS, "the SD value");
        bytes.truncate(usize::try_from(length).unwrap());
        bytes
    }

    /// Sets its `SD` value to `bytes`, `REG_BINARY`; `Err` with the Win32 code.
    fn set_sd(&self, bytes: &[u8]) -> Result<(), u32> {
        // SAFETY: a live key; the name and `bytes` outlive the call.
        let set = unsafe { RegSetValueExW(self.0, w!("SD"), None, REG_BINARY, Some(bytes)) };
        if set == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(set.0)
        }
    }
}

impl Drop for CacheKey {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// The self-relative descriptor `sddl` describes, as bytes.
fn sd_bytes(sddl: &str) -> Vec<u8> {
    let text = HSTRING::from(sddl);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
    // out-pointer, freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(text.as_ptr()),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .unwrap();
    // SAFETY: a valid self-relative descriptor from the conversion.
    let length = unsafe { GetSecurityDescriptorLength(descriptor) };
    // SAFETY: the descriptor is `length` bytes long and live until freed below.
    let bytes = unsafe {
        std::slice::from_raw_parts(descriptor.0.cast::<u8>(), usize::try_from(length).unwrap())
    }
    .to_vec();
    // SAFETY: allocated by the conversion with LocalAlloc, freed once, here.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    bytes
}

/// What the judging function makes of the fixture's task, its user the pool.
fn judged(fixture: &Fixture, sweeper: &mut WindowsSweeper) -> Result<(), String> {
    let pool = [fixture.user.sid.clone()];
    let names = Names::of(sweeper, &pool);
    let path = format!(r"\{}", fixture.name);
    not_pool_by_path(sweeper, &path, &pool, &names, &mut BTreeMap::new())
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_an_ownerless_or_system_owned_cache_sd_keeps_a_pool_users_task_at_56() {
    if !gate("elevated_an_ownerless_or_system_owned_cache_sd_keeps_a_pool_users_task_at_56") {
        return;
    }
    let fixture = Fixture::new("ForgedSd");
    let path = format!(r"\{}", fixture.name);
    let mut sweeper = WindowsSweeper::new();
    let owner = sweeper.task_cache_owner(&path);
    assert_eq!(owner.as_ref(), Ok(&fixture.user.sid), "as registered");
    assert!(judged(&fixture, &mut sweeper).is_err(), "as registered");
    // 3. As this administrator: the SD value ownerless, then naming SYSTEM its owner.
    for (sddl, system) in [
        (OWNERLESS, false),
        ("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)", true),
    ] {
        fixture.set_sd(&sd_bytes(sddl));
        let owner = sweeper.task_cache_owner(&path);
        eprintln!("3. {sddl}: its cache key's owner {owner:?}");
        let want = system.then(|| Sid::parse("S-1-5-18").unwrap());
        assert_eq!(owner.ok(), want, "{sddl}");
        let judged = judged(&fixture, &mut sweeper);
        eprintln!("   judged: {judged:?}");
        assert!(judged.is_err(), "{sddl}: the task was skipped");
        if !system {
            assert_eq!(judged, Err(ODD_SD.to_owned()), "{sddl}");
        }
    }
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "rewrites the test task's own file, as the administrator this test runs as"
)]
fn elevated_a_task_file_rewritten_to_system_and_relocked_doesnt_pass_an_odd_cache_sd() {
    if !gate("elevated_a_task_file_rewritten_to_system_and_relocked_doesnt_pass_an_odd_cache_sd") {
        return;
    }
    let fixture = Fixture::new("Relocked");
    // 3. As this administrator: the task file names SYSTEM, owned by Administrators, with a
    // DACL that gives the test user nothing, as a registrant could leave it after naming
    // SYSTEM and dropping its own write.
    let file = PathBuf::from(format!(r"{}\Tasks\{}", system32(), fixture.name));
    let bytes = std::fs::read(&file).unwrap();
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let text = String::from_utf16(&units).unwrap();
    let (head, rest) = text
        .split_once("<UserId>")
        .expect("the file names a UserId");
    let (_, tail) = rest.split_once("</UserId>").unwrap();
    let rewritten = format!("{head}<UserId>S-1-5-18</UserId>{tail}");
    let out: Vec<u8> = rewritten
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    std::fs::write(&file, out).unwrap();
    set_owner(&file, &Sid::parse("S-1-5-32-544").unwrap()).unwrap();
    protect_with(&file, "D:P(A;;FA;;;SY)(A;;FA;;;BA)").unwrap();
    // 4. The file now passes the trust rules and names SYSTEM: what an odd SD was skipped on.
    let mut sweeper = WindowsSweeper::new();
    let pool = [fixture.user.sid.clone()];
    let parts = task_file::parts("\\", &fixture.name).unwrap();
    let read = sweeper.read_task_file(&parts).unwrap();
    assert_eq!(
        trusted(&read.security, &pool),
        Ok(()),
        "the file is trusted"
    );
    let principal = task_file::principal_of(&read.bytes).unwrap();
    assert_eq!(principal.name(), Some("S-1-5-18"));
    // 5. With an odd cache SD (ownerless, unparsable, over its cap): 56, not skipped.
    let over_cap = vec![0_u8; 256 * 1024 + 1];
    for (what, sd) in [
        ("ownerless", sd_bytes(OWNERLESS)),
        ("unparsable", vec![0_u8; 8]),
        ("over its cap", over_cap),
    ] {
        fixture.set_sd(&sd);
        let judged = judged(&fixture, &mut sweeper);
        eprintln!("5. {what}: judged {judged:?}");
        assert_eq!(
            judged,
            Err(ODD_SD.to_owned()),
            "{what}: the task was skipped"
        );
    }
}
