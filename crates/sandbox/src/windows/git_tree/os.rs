//! [`TreeFs`] on Windows: the Git trees' effects, as SYSTEM or an elevated administrator.
//! The files and folders are in `os/files.rs` (every one opened as itself, a folder of a
//! tree's own chain relative to its parent's handle) and `os/level.rs` (a tree's folders made
//! and given their owner and DACL); the record, the processes and the Event Log are here.
#![expect(
    unsafe_code,
    reason = "FFI to the registry calls that read the profiles folder and write the record"
)]

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_CREATE_SUB_KEY, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE,
    KEY_SET_VALUE, KEY_WOW64_64KEY, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW,
    RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::{Account, Entry, FileId, Recorded, Refused, TreeFs, build};
use crate::windows::acl::privilege::Privileges;
use crate::windows::admin_copy::{Found, os as admin_os};
use crate::windows::record;
use crate::windows::refresh::{event_log, pe_version};
use crate::windows::setup::preflight::account_name;
use crate::windows::sids::Sid;

mod files;
mod level;

/// The record key under `HKLM`, as `setup/record_write.rs` names it.
const RECORD_KEY: &str = r"SOFTWARE\Clauseo\Sandbox";
/// `HKLM\...\ProfileList`, which names the profiles folder.
const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
/// `DELETE`, which `RegDeleteTreeW` needs on the record key.
const KEY_DELETE: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0001_0000);
/// The most of an exe [`TreeFs::file_version`] reads.
const VERSION_CAP: u64 = 128 * 1024 * 1024;

/// The machine's Git trees.
pub struct WindowsTrees;

/// Builds or keeps every account's tree and records each ([`build::ensure`]), on this machine.
pub fn ensure_for(accounts: &[Account], group: &Sid) -> Vec<(Sid, Recorded)> {
    build::ensure(&mut WindowsTrees, accounts, group)
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
    ensure_for(&accounts, &group);
}

/// Deletes every tree and the record ([`build::remove`]), on this machine.
pub fn remove_all() -> Result<(), String> {
    build::remove(&mut WindowsTrees)
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

    fn volume(&mut self, path: &Path) -> Result<u64, String> {
        files::volume(path)
    }

    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused> {
        files::walk(root)
    }

    fn found(&mut self, path: &Path) -> Result<Found, String> {
        admin_os::found(path).map_err(|error| format!("reading {}: {error}", path.display()))
    }

    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused> {
        level::level(path, sddl)
    }

    fn entries(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        files::entries(dir)
    }

    fn make_dir(&mut self, path: &Path) -> Result<(), String> {
        files::make_dir(path)
    }

    fn hard_link(&mut self, existing: &Path, link: &Path) -> Result<(), String> {
        files::hard_link(existing, link)
    }

    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        files::copy_file(from, to)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        files::rename(from, to)
    }

    fn remove_tree(&mut self, path: &Path) -> Result<(), String> {
        files::remove_tree(path)
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

    fn write_record(&mut self, values: &[(Sid, String)]) -> Result<(), String> {
        let Some(record) = open_record_key()? else {
            return Err(
                "the record's key is missing, so the Git trees weren't recorded".to_owned(),
            );
        };
        delete_subkey(&record)?;
        let subkey = HSTRING::from(super::RECORD_SUBKEY);
        let mut made = HKEY::default();
        // SAFETY: the record key is open with KEY_CREATE_SUB_KEY; the subkey name is
        // NUL-terminated; no security attributes are passed, so the subkey inherits the record
        // key's DACL; `made` is a live out-pointer, closed by `Key`.
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
        for (sid, text) in values {
            let data: Vec<u8> = text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            let name = HSTRING::from(sid.as_str());
            // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data`
            // is a live slice holding the NUL-terminated UTF-16 text.
            let set = unsafe { RegSetValueExW(made.0, &name, None, REG_SZ, Some(&data)) };
            check("RegSetValueExW", set)?;
        }
        Ok(())
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
