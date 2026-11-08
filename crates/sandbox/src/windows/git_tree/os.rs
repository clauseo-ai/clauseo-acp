//! [`TreeFs`] on Windows: the Git trees' effects, as SYSTEM or an elevated administrator.
//! The files and folders are in `os/`, every one opened as itself and, below a known folder,
//! relative to its parent's handle: `files.rs` reads, `walk.rs` walks, `put.rs` copies, links
//! and writes the marker, `delete.rs` deletes, `level.rs` gives a tree's folders their owner
//! and DACL, and `nt.rs` holds the NT calls they share. The record, the processes and the
//! Event Log are here.
#![expect(
    unsafe_code,
    reason = "FFI to the registry calls that read the profiles folder and write the record"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_CREATE_SUB_KEY, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE,
    KEY_SET_VALUE, KEY_WOW64_64KEY, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW,
    RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use super::{Account, Entry, FileId, Inspected, Recorded, Refused, TreeFs, build, retire};
use crate::windows::acl::privilege::Privileges;
use crate::windows::admin_copy::{Found, os as admin_os};
use crate::windows::record;
use crate::windows::refresh::{event_log, pe_version};
use crate::windows::runner::RUNNER_EXE;
use crate::windows::setup::preflight::account_name;
use crate::windows::sids::Sid;

mod delete;
mod files;
mod level;
mod nt;
mod put;
mod stub;
mod walk;

/// The record key under `HKLM`, as `setup/record_write.rs` names it.
const RECORD_KEY: &str = r"SOFTWARE\Clauseo\Sandbox";
/// `HKLM\...\ProfileList`, which names the profiles folder.
const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
/// `DELETE`, which `RegDeleteTreeW` needs on the record key.
const KEY_DELETE: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0001_0000);
/// The most of an exe [`TreeFs::file_version`] reads.
const VERSION_CAP: u64 = 128 * 1024 * 1024;
/// How many names [`TreeFs::unique`] made without randomness, which keeps each its own.
static UNIQUE_CALLS: AtomicU64 = AtomicU64::new(0);
/// The longest registry value name, in UTF-16 units, and its NUL.
const VALUE_NAME_MAX: usize = 16_384;

/// The machine's Git trees.
pub struct WindowsTrees;

/// Builds or keeps every account's tree and records each, and places the Git Bash stub from
/// `runner` ([`build::ensure_with`]), on this machine.
pub fn ensure_for(
    accounts: &[Account],
    group: &Sid,
    runner: Option<&Path>,
) -> Vec<(Sid, Recorded)> {
    build::ensure_with(&mut WindowsTrees, accounts, group, runner)
}

/// The runner the set-up copies the Git Bash stub from: [`RUNNER_EXE`] beside this exe (a
/// set-up run from the install folder), else in the record's `UserInstallDir` (the install
/// folder `--adopt` recorded; the admin copy has no runner beside it), each a regular file
/// there; `None` when neither is. The copy opens it as itself (`stub.rs`).
pub fn runner_for_stub() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let install = record::user_install_dir().ok().flatten();
    [beside, install]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(RUNNER_EXE))
        .find(|path| matches!(admin_os::found(path), Ok(Found::File)))
}

/// [`ensure_for`] the pool the machine record names, for the SYSTEM refresh: nothing while a
/// removal is under way or when that can't be read; the pool's SIDs and group read from the
/// record, each SID named (`LookupAccountSidW`), one with no name passed over. A pool or group
/// that can't be read, or a name that can't be looked up, logs one line and changes nothing:
/// a shorter pool would delete trees.
pub fn ensure_from_record() {
    if !matches!(record::sandbox_removing(), Ok(false)) {
        return;
    }
    let Ok(pool) = record::sandbox_pool_sids() else {
        event_log(
            true,
            "Git Bash trees: the sandbox users' list can't be read",
        );
        return;
    };
    let group = record::sandbox_group_sid()
        .ok()
        .flatten()
        .and_then(|text| Sid::parse(&text).ok());
    let Some(group) = group else {
        event_log(
            true,
            "Git Bash trees: the sandbox users' group can't be read",
        );
        return;
    };
    let mut accounts = Vec::new();
    for sid in pool {
        match account_name(&sid) {
            Ok(Some(name)) => accounts.push(Account { name, sid }),
            Ok(None) => {}
            // Left out, its trees would be deleted: the next run does them all.
            Err(_) => {
                event_log(true, "Git Bash trees: a sandbox user's name can't be read");
                return;
            }
        }
    }
    // The refresh has no runner beside the admin copy: the stub there stays.
    ensure_for(&accounts, &group, None);
}

/// Deletes every tree and the record ([`retire::remove`]), on this machine.
pub fn remove_all() -> Result<(), String> {
    retire::remove(&mut WindowsTrees)
}

/// Checks the trees' folder `git` an adopted install brings ([`retire::check_adopted`]), on
/// this machine.
pub fn check_adopted_here(git: &Path) {
    retire::check_adopted(&mut WindowsTrees, git);
}

/// Deletes the trees' folder `git` by a plain delete ([`retire::remove_folder`]), on this
/// machine: the uninstaller's clear of the admin folder, which never takes `git\` back.
pub fn remove_folder_here(git: &Path) {
    retire::remove_folder(&mut WindowsTrees, git);
}

impl TreeFs for WindowsTrees {
    fn admin_dir(&mut self) -> Result<PathBuf, String> {
        admin_os::admin_dir_checked()
            .map_err(|blocked| format!("the admin folder's chain was refused ({blocked:?})"))
    }

    fn registry_install(&mut self) -> Result<Option<String>, String> {
        record::read_string(w!(r"SOFTWARE\GitForWindows"), w!("InstallPath"))
            .map_err(|error| format!("reading GitForWindows' InstallPath: {error}"))
    }

    fn program_files(&mut self) -> Result<PathBuf, String> {
        admin_os::program_files().map_err(|error| format!("finding Program Files: {error}"))
    }

    fn profiles_dir(&mut self) -> Result<PathBuf, String> {
        reg_text(PROFILE_LIST, "ProfilesDirectory")?
            .map(PathBuf::from)
            .ok_or_else(|| "ProfileList has no ProfilesDirectory".to_owned())
    }

    fn is_network(&mut self, path: &Path) -> Result<bool, String> {
        files::is_network(path)
    }

    fn plain_chain(&mut self, dir: &Path) -> Result<(), Refused> {
        files::plain_chain(dir)
    }

    fn admin_only(&mut self, dir: &Path) -> Result<bool, String> {
        files::admin_only(dir)
    }

    fn file_id(&mut self, path: &Path) -> Result<Option<FileId>, Refused> {
        files::file_id(path)
    }

    fn file_version(&mut self, path: &Path) -> Result<Option<String>, String> {
        let image = files::read_capped(path, VERSION_CAP)?;
        Ok(pe_version::file_version(&image).map(pe_version::dotted))
    }

    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused> {
        walk::walk(root)
    }

    fn found(&mut self, path: &Path) -> Result<Found, String> {
        admin_os::found(path).map_err(|error| format!("reading {}: {error}", path.display()))
    }

    fn inspect(&mut self, path: &Path) -> Result<Option<Inspected>, String> {
        files::inspect(path)
    }

    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused> {
        level::level(path, sddl)
    }

    fn entries(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        files::entries(dir)
    }

    fn copy_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
        put::copy_tree(from, entries, to)
    }

    fn link_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
        put::link_tree(from, entries, to)
    }

    fn read_marker(&mut self, dir: &Path) -> Result<Option<String>, String> {
        files::read_marker(dir)
    }

    fn write_marker(&mut self, dir: &Path, text: &str, sddl: &str) -> Result<(), String> {
        put::write_marker(dir, text, sddl)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        files::rename(from, to)
    }

    fn remove_tree(&mut self, path: &Path) -> Result<(), String> {
        delete::remove_tree(path)
    }

    fn delete_at_reboot(&mut self, path: &Path) -> Result<(), String> {
        delete::delete_at_reboot(path)
    }

    fn place_file(
        &mut self,
        from: &Path,
        dir: &Path,
        temp: &str,
        name: &str,
    ) -> Result<(), Refused> {
        stub::place_file(from, dir, temp, name)
    }

    fn unique(&mut self) -> String {
        let mut bytes = [0_u8; 8];
        if getrandom::fill(&mut bytes).is_ok() {
            return crate::windows::refresh::hex(&bytes);
        }
        // No randomness: the time, the process ID and a count still make a name no other
        // call made.
        // SAFETY: the call takes nothing and returns the time by value.
        let time = unsafe { GetSystemTimePreciseAsFileTime() };
        let time = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        let pid = u64::from(std::process::id()).rotate_left(40);
        let count = UNIQUE_CALLS.fetch_add(1, Ordering::Relaxed);
        format!("{:016x}", time ^ pid ^ count.rotate_left(20))
    }

    fn active(&mut self, account: &Sid) -> Result<bool, String> {
        // A pool user's process grants nothing to Administrators; `SeDebugPrivilege` (an
        // elevated or SYSTEM token holds it) opens it all the same. Without it, a process that
        // can't be opened shows no user, as below.
        let _debug = Privileges::enable(&["SeDebugPrivilege"]).ok();
        let seen = crate::windows::setup::processes::running()
            .map_err(|error| format!("listing the processes: {error}"))?;
        // A process whose user can't be read is passed over: a protected process, which a pool
        // user can't start, or one gone since the list.
        Ok(seen
            .iter()
            .any(|process| process.user.as_ref() == Some(account)))
    }

    fn set_record_value(&mut self, account: &Sid, text: &str) -> Result<(), String> {
        let Some(record) = open_record_key()? else {
            return Err(
                "the record's key is missing, so the Git trees weren't recorded".to_owned(),
            );
        };
        let subkey = HSTRING::from(super::RECORD_SUBKEY);
        let mut made = HKEY::default();
        // SAFETY: the record key is open with KEY_CREATE_SUB_KEY; the subkey name is
        // NUL-terminated; no security attributes are passed, so a subkey made here inherits the
        // record key's DACL; `made` is a live out-pointer, closed by `Key`.
        let created = unsafe {
            RegCreateKeyExW(
                record.0,
                &subkey,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &raw mut made,
                None,
            )
        };
        check("RegCreateKeyExW", created)?;
        let made = Key(made);
        let data: Vec<u8> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let name = HSTRING::from(account.as_str());
        // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data` is
        // a live slice holding the NUL-terminated UTF-16 text. One call replaces the value.
        let set = unsafe { RegSetValueExW(made.0, &name, None, REG_SZ, Some(&data)) };
        check("RegSetValueExW", set)
    }

    fn record_names(&mut self) -> Result<Vec<String>, String> {
        let Some(subkey) = open_subkey(KEY_QUERY_VALUE)? else {
            return Ok(Vec::new());
        };
        let mut names = Vec::new();
        let mut buffer = vec![0_u16; VALUE_NAME_MAX];
        for index in 0.. {
            let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
            // SAFETY: the key is open with KEY_QUERY_VALUE; `buffer` is writable for `length`
            // UTF-16 units and `length` a live in-out pointer; no type or data is asked for.
            let read = unsafe {
                RegEnumValueW(
                    subkey.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if read == ERROR_NO_MORE_ITEMS {
                break;
            }
            check("RegEnumValueW", read)?;
            let length = usize::try_from(length).unwrap_or(0);
            names.push(String::from_utf16_lossy(
                buffer.get(..length).unwrap_or_default(),
            ));
        }
        Ok(names)
    }

    fn delete_record_value(&mut self, name: &str) -> Result<(), String> {
        let Some(subkey) = open_subkey(KEY_SET_VALUE)? else {
            return Ok(());
        };
        let name = HSTRING::from(name);
        // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated.
        let deleted = unsafe { RegDeleteValueW(subkey.0, &name) };
        if missing(deleted) {
            return Ok(());
        }
        check("RegDeleteValueW", deleted)
    }

    fn delete_record(&mut self) -> Result<(), String> {
        match open_record_key()? {
            Some(record) => delete_subkey(&record),
            None => Ok(()),
        }
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW or RegCreateKeyExW and is closed once, here.
        // A failed close leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view) opened to make, delete and fill its
/// subkeys; `None` when it is missing. It is never made here.
fn open_record_key() -> Result<Option<Key>, String> {
    let subkey = HSTRING::from(RECORD_KEY);
    let mut opened = HKEY::default();
    let access = KEY_CREATE_SUB_KEY
        | KEY_ENUMERATE_SUB_KEYS
        | KEY_QUERY_VALUE
        | KEY_SET_VALUE
        | KEY_DELETE
        | KEY_WOW64_64KEY;
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let open = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &subkey, None, access, &raw mut opened) };
    if missing(open) {
        return Ok(None);
    }
    check("RegOpenKeyExW", open)?;
    Ok(Some(Key(opened)))
}

/// The record's subkey (`HKLM\SOFTWARE\Clauseo\Sandbox\GitTrees`, 64-bit view) opened for
/// `access`; `None` when it, or the record key, is missing. It is never made here.
fn open_subkey(access: REG_SAM_FLAGS) -> Result<Option<Key>, String> {
    let subkey = HSTRING::from(format!(r"{RECORD_KEY}\{}", super::RECORD_SUBKEY));
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            access | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    if missing(open) {
        return Ok(None);
    }
    check("RegOpenKeyExW", open)?;
    Ok(Some(Key(opened)))
}

/// Deletes the record's subkey under the open record key, with all in it; missing is `Ok`.
fn delete_subkey(record: &Key) -> Result<(), String> {
    let subkey = HSTRING::from(super::RECORD_SUBKEY);
    // SAFETY: the key is open with DELETE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE and
    // KEY_SET_VALUE; the subkey name is NUL-terminated.
    let deleted = unsafe { RegDeleteTreeW(record.0, &subkey) };
    if missing(deleted) {
        return Ok(());
    }
    check("RegDeleteTreeW", deleted)
}

/// The `REG_SZ` or `REG_EXPAND_SZ` (expanded) `value` of `HKLM\<subkey>`, 64-bit view;
/// `None` when the key or the value is missing.
fn reg_text(subkey: &str, value: &str) -> Result<Option<String>, String> {
    let (subkey, value) = (HSTRING::from(subkey), HSTRING::from(value));
    let flags = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0_u32;
    // SAFETY: the names are NUL-terminated and outlive the call; no data pointer asks only for
    // the size, written to `size`.
    let sized = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if missing(sized) {
        return Ok(None);
    }
    check("RegGetValueW", sized)?;
    let mut buffer = vec![0_u16; usize::try_from(size).unwrap_or(0).div_ceil(2).max(1)];
    let mut written = u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX);
    // SAFETY: as above; `buffer` is live, writable and `written` bytes long.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut written),
        )
    };
    if missing(read) {
        return Ok(None);
    }
    check("RegGetValueW", read)?;
    let text = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_not_utf16| "a registry value isn't text".to_owned())
}

/// Whether a registry call found no key or value there.
fn missing(code: WIN32_ERROR) -> bool {
    code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND
}

/// A registry call's result as `Ok`, or its words naming `call`.
fn check(call: &str, code: WIN32_ERROR) -> Result<(), String> {
    if code.is_ok() {
        Ok(())
    } else {
        Err(format!("{call} failed (error {})", code.0))
    }
}
