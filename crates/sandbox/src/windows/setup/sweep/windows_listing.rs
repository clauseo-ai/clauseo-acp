//! The two listings step 4's cross-check reads (`unlisted.rs`), for
//! [`Sweeper::task_file_paths`] and [`Sweeper::task_cache_paths`]. Neither deletes or writes
//! anything; errors name the step, never a folder or task.
//!
//! - Every task file below `%SystemRoot%\System32\Tasks`, walked as `windows_task_file.rs`
//!   opens: the system folder opened as itself, `Tasks` and each folder below it by handle
//!   relative to the one above (`OBJ_DONT_REPARSE`, backup intent), each refused if a reparse
//!   point or not a folder, and listed through that handle (`FileIdBothDirectoryInfo`). A name
//!   the listing says is a reparse point, or that isn't UTF-16 text, fails the walk: what it
//!   hides can't be checked. `SeBackupPrivilege` is enabled for the walk alone.
//! - Every task under the Task Scheduler's cache,
//!   `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree` (a key holding
//!   an `Id` value; a folder's key holds none), opened read-only in the 64-bit view. A task's
//!   key also holds its security descriptor, the `SD` value, which gives a hidden task's owner
//!   ([`Sweeper::task_cache_owner`]), and its `Id` value, which names its keys under
//!   `TaskCache\{Tasks,Plain,Logon,Boot}` when it is a task and not an orphan
//!   ([`Sweeper::task_cache_entries`]). Each of those is opened as the `Tree` keys are.
//!
//! [`Sweeper::task_file_paths`]: super::Sweeper::task_file_paths
//! [`Sweeper::task_cache_paths`]: super::Sweeper::task_cache_paths
//! [`Sweeper::task_cache_owner`]: super::Sweeper::task_cache_owner
//! [`Sweeper::task_cache_entries`]: super::Sweeper::task_cache_entries
#![expect(
    unsafe_code,
    reason = "the registry calls that list and read the Task Scheduler's cache"
)]

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_WOW64_64KEY, REG_BINARY,
    REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};
use windows::core::{HSTRING, PWSTR, w};

use super::{FOLDERS_CAP, Failed, LISTED_CAP};
use crate::windows::acl::object::Object;
use crate::windows::acl::privilege::Privileges;
use crate::windows::acl_bytes::owner_sid;
use crate::windows::sids::Sid;

/// What the file walk enables.
const BACKUP: [&str; 1] = ["SeBackupPrivilege"];

/// The most a task's `SD` value is read to: a descriptor larger is refused.
const SD_CAP: usize = 64 * 1024;

/// The Task Scheduler's cache of task paths, below `HKEY_LOCAL_MACHINE`.
const TREE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree";

/// The Task Scheduler's cache, below `HKEY_LOCAL_MACHINE`, and the keys in it that hold a
/// task's entry by its `Id`.
const CACHE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache";
const ENTRIES: [&str; 4] = ["Tasks", "Plain", "Logon", "Boot"];

/// A GUID in braces, as an `Id` value holds it: 38 characters.
const GUID_LEN: usize = 38;

/// Every task file below `Tasks` in the system folder `system`, as its task's path
/// (`\A\B\name`), as the module says.
pub(super) fn task_files(system: &Path) -> Result<Vec<String>, Failed> {
    let _backup = Privileges::enable(&BACKUP).map_err(|error| error.to_string())?;
    let root = Object::open_for(system, 0)
        .map_err(|error| format!("the system folder couldn't be opened ({error})"))?;
    if root.reparse() || !root.folder {
        return Err("the system folder isn't a plain folder".to_owned());
    }
    let tasks = listable(&root, &wide("Tasks"))?;
    let mut stack = vec![(String::new(), tasks)];
    // Counted, and capped, before a folder's handle is opened: the stack holds one open handle
    // per folder in it, so a wide folder can't open more than the cap.
    let mut folders = 1_usize;
    let mut paths = Vec::new();
    while let Some((path, folder)) = stack.pop() {
        let children = folder
            .children()
            .map_err(|error| format!("a folder below Tasks couldn't be listed ({error})"))?;
        for child in children {
            if child.reparse {
                return Err("a name below Tasks is a reparse point".to_owned());
            }
            let name = String::from_utf16(&child.name)
                .map_err(|_not_text| "a name below Tasks isn't UTF-16 text".to_owned())?;
            let at = format!("{path}\\{name}");
            if child.folder {
                folders += 1;
                if folders > FOLDERS_CAP {
                    return Err(format!("more than {FOLDERS_CAP} folders below Tasks"));
                }
                stack.push((at, listable(&folder, &child.name)?));
            } else {
                paths.push(at);
                if paths.len() > LISTED_CAP {
                    return Err(format!("more than {LISTED_CAP} task files"));
                }
            }
        }
    }
    Ok(paths)
}

/// The folder `name` in `folder`, opened by handle to be listed; `Err` if it is a reparse
/// point or not a folder.
fn listable(folder: &Object, name: &[u16]) -> Result<Object, Failed> {
    let next = folder
        .open_child_for(name, FILE_LIST_DIRECTORY.0)
        .map_err(|error| {
            format!("a folder below the system folder couldn't be opened ({error})")
        })?;
    if next.reparse() || !next.folder {
        return Err("a folder below the system folder isn't a plain folder".to_owned());
    }
    Ok(next)
}

/// `part` in UTF-16, with no NUL.
fn wide(part: &str) -> Vec<u16> {
    part.encode_utf16().collect()
}

/// A registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW and is closed once, here. A failed close
        // leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    /// `subkey` below `root`, read-only, in the 64-bit view.
    fn open(root: HKEY, subkey: &HSTRING) -> Result<Key, Failed> {
        Key::find(root, subkey)?
            .ok_or_else(|| "a key of the Task Scheduler's cache couldn't be opened (2)".to_owned())
    }

    /// [`Key::open`], `None` when the key is missing.
    fn find(root: HKEY, subkey: &HSTRING) -> Result<Option<Key>, Failed> {
        let access = KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | KEY_WOW64_64KEY;
        let mut key = HKEY::default();
        // SAFETY: `subkey` is NUL-terminated and lives for the call; `key` is a live
        // out-pointer, and the key it receives is owned by the `Key` returned.
        let opened = unsafe { RegOpenKeyExW(root, subkey, None, access, &raw mut key) };
        if opened == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if opened != ERROR_SUCCESS {
            return Err(format!(
                "a key of the Task Scheduler's cache couldn't be opened ({})",
                opened.0
            ));
        }
        Ok(Some(Key(key)))
    }

    /// The names of the keys directly below this one.
    fn subkeys(&self) -> Result<Vec<String>, Failed> {
        let mut names = Vec::new();
        for at in 0_u32.. {
            // A key's name is at most 255 characters.
            let mut name = [0_u16; 256];
            let mut length = 256_u32;
            // SAFETY: the key is open; `name` is writable for `length` characters, and the
            // call writes the length it used back.
            let listed = unsafe {
                RegEnumKeyExW(
                    self.0,
                    at,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if listed == ERROR_NO_MORE_ITEMS {
                break;
            }
            if listed != ERROR_SUCCESS {
                return Err(format!(
                    "a key of the Task Scheduler's cache couldn't be listed ({})",
                    listed.0
                ));
            }
            let used = name.get(..length as usize).unwrap_or(&name);
            let text = String::from_utf16(used).map_err(|_not_text| {
                "a key of the Task Scheduler's cache isn't UTF-16 text".to_owned()
            })?;
            names.push(text);
        }
        Ok(names)
    }

    /// Whether this key holds an `Id` value: a task's key.
    fn is_task(&self) -> Result<bool, Failed> {
        // SAFETY: the key is open and the name a NUL-terminated literal; nothing is read back.
        let found = unsafe { RegQueryValueExW(self.0, w!("Id"), None, None, None, None) };
        if found == ERROR_SUCCESS {
            Ok(true)
        } else if found == ERROR_FILE_NOT_FOUND {
            Ok(false)
        } else {
            Err(format!(
                "a value of the Task Scheduler's cache couldn't be read ({})",
                found.0
            ))
        }
    }

    /// This key's `Id` value, which must be `REG_SZ` and a GUID in braces: anything else could
    /// name another key below `Tasks` (a `\`, say) and is refused.
    fn id(&self) -> Result<String, Failed> {
        // Room for the GUID, its NUL and one character more, so a longer value fails the read.
        let mut text = [0_u16; GUID_LEN + 2];
        let mut length = u32::try_from(size_of_val(&text)).unwrap_or(0);
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: the key is open and the name a NUL-terminated literal; `text` is writable
        // for `length` bytes, and the call writes the type and the length it used back.
        let read = unsafe {
            RegQueryValueExW(
                self.0,
                w!("Id"),
                None,
                Some(&raw mut kind),
                Some(text.as_mut_ptr().cast()),
                Some(&raw mut length),
            )
        };
        if read != ERROR_SUCCESS {
            return Err(format!(
                "a task's Id in the Task Scheduler's cache couldn't be read ({})",
                read.0
            ));
        }
        if kind != REG_SZ {
            return Err("a task's Id in the Task Scheduler's cache isn't text".to_owned());
        }
        let used = text.get(..length as usize / 2).unwrap_or(&text);
        let id = String::from_utf16(used)
            .map_err(|_not_text| "a task's Id in the Task Scheduler's cache isn't UTF-16 text")?;
        let id = id.strip_suffix('\0').unwrap_or(&id);
        if !is_guid(id) {
            return Err("a task's Id in the Task Scheduler's cache isn't a GUID".to_owned());
        }
        Ok(id.to_owned())
    }

    /// This key's `SD` value, which must be `REG_BINARY`, at most [`SD_CAP`] bytes.
    fn descriptor(&self) -> Result<Vec<u8>, Failed> {
        let mut bytes = vec![0_u8; SD_CAP];
        let mut length = u32::try_from(SD_CAP).unwrap_or(u32::MAX);
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: the key is open and the name a NUL-terminated literal; `bytes` is writable
        // for `length` bytes, and the call writes the type and the length it used back.
        let read = unsafe {
            RegQueryValueExW(
                self.0,
                w!("SD"),
                None,
                Some(&raw mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&raw mut length),
            )
        };
        if read != ERROR_SUCCESS {
            return Err(format!(
                "a task's descriptor in the Task Scheduler's cache couldn't be read ({})",
                read.0
            ));
        }
        if kind != REG_BINARY {
            return Err(
                "a task's descriptor in the Task Scheduler's cache isn't binary".to_owned(),
            );
        }
        bytes.truncate(length as usize);
        Ok(bytes)
    }
}

/// The owner of the task at `path` (`\A\name`) that its key in the Task Scheduler's cache
/// holds in its `SD` value, as the module says; `Err` when the key or value is missing, can't
/// be read, or names no owner.
pub(super) fn task_cache_owner(path: &str) -> Result<Sid, Failed> {
    let key = Key::open(HKEY_LOCAL_MACHINE, &HSTRING::from(format!("{TREE}{path}")))?;
    owner_sid(&key.descriptor()?).ok_or_else(|| {
        "a task's descriptor in the Task Scheduler's cache names no owner".to_owned()
    })
}

/// Whether `id` is a GUID in braces: `{` 8, 4, 4, 4 and 12 hex digits joined by `-`, `}`.
fn is_guid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == GUID_LEN
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            0 => *byte == b'{',
            37 => *byte == b'}',
            9 | 14 | 19 | 24 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// Whether the cache holds a task for the key at `path` (`\A\name`) past that key, as
/// [`Sweeper::task_cache_entries`] says: its `Id` read, then each key under
/// `TaskCache\{Tasks,Plain,Logon,Boot}` by that `Id` opened from `HKEY_LOCAL_MACHINE` by
/// its whole path, as the `Tree` keys are.
///
/// [`Sweeper::task_cache_entries`]: super::Sweeper::task_cache_entries
pub(super) fn task_cache_entries(path: &str) -> Result<bool, Failed> {
    let id = Key::open(HKEY_LOCAL_MACHINE, &HSTRING::from(format!("{TREE}{path}")))?.id()?;
    for entries in ENTRIES {
        let at = HSTRING::from(format!("{CACHE}\\{entries}\\{id}"));
        if Key::find(HKEY_LOCAL_MACHINE, &at)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Every task the Task Scheduler's cache lists, as its path (`\A\B\name`), as the module says.
/// Each key is opened from `HKEY_LOCAL_MACHINE` by its whole path and closed before the next.
pub(super) fn task_cache() -> Result<Vec<String>, Failed> {
    let mut stack = vec![String::new()];
    let mut keys = 0_usize;
    let mut paths = Vec::new();
    while let Some(path) = stack.pop() {
        let key = Key::open(HKEY_LOCAL_MACHINE, &HSTRING::from(format!("{TREE}{path}")))?;
        if !path.is_empty() && key.is_task()? {
            paths.push(path.clone());
        }
        for name in key.subkeys()? {
            keys += 1;
            if keys > LISTED_CAP {
                return Err(format!(
                    "more than {LISTED_CAP} keys in the Task Scheduler's cache"
                ));
            }
            stack.push(format!("{path}\\{name}"));
        }
    }
    Ok(paths)
}
