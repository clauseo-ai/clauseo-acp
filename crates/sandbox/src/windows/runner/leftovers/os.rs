//! The removal's calls on Windows, made as the pool user: the per-user COM class keys and the
//! `Run` and `RunOnce` values of its own `HKEY_CURRENT_USER` (its hive, loaded by
//! `LOGON_WITH_PROFILE`), its Startup folder, and Task Scheduler and BITS (`com.rs`). COM is
//! initialised only when a kind reached over it is listed, which the inner runner never does.
//!
//! The Startup folder is `FOLDERID_Startup` for this token, which must lie in the token's profile
//! folder: the known folder's path comes from the user's own registry, which a process of the
//! pool user can point anywhere. It is reached from the profile folder one name at a time,
//! relative to each folder's handle with no reparse point followed (`Object::open_child`); a link
//! on the way fails the list. Each entry is deleted on its own handle, opened as itself (a link is
//! deleted, never what it names); a real folder is emptied the same way first.
#![expect(
    unsafe_code,
    reason = "FFI to the registry, the known folder and the file deletes"
)]

use std::io;
use std::path::Path;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::Networking::BackgroundIntelligentTransferService::IBackgroundCopyJob;
use windows::Win32::Security::{LookupAccountSidW, SID_NAME_USE};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
    FILE_DISPOSITION_INFO_EX_FLAGS, FILE_READ_ATTRIBUTES, FileDispositionInfoEx, SYNCHRONIZE,
    SetFileInformationByHandle,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, RegCloseKey,
    RegDeleteTreeW, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW,
};
use windows::Win32::System::TaskScheduler::ITaskFolder;
use windows::Win32::UI::Shell::{FOLDERID_Startup, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath};
use windows::core::{BSTR, PCWSTR, PWSTR, w};

use super::{Kind, Leftovers, below};
use crate::windows::acl::object::{Handle, Object, open_at, os_error};
use crate::windows::sids::Sid;
use crate::windows::token::os::{LocalSid, profile_folder};

mod com;

/// The `Run` and `RunOnce` keys under `HKEY_CURRENT_USER`.
const RUN_KEYS: [PCWSTR; 2] = [
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
];
/// The longest value name, and its NUL.
const NAME_CAP: usize = 16_384;
/// How deep a folder in the Startup folder is emptied.
const FOLDER_DEPTH: usize = 16;
/// Where the per-user classes are, under `HKEY_CURRENT_USER`.
const CLASSES: &str = "Software\\Classes";
/// The class keys deleted whole, under [`CLASSES`]: the 64-bit view's, then the 32-bit one's.
const CLASS_KEYS: [&str; 8] = [
    "CLSID",
    "Interface",
    "AppID",
    "TypeLib",
    "Wow6432Node\\CLSID",
    "Wow6432Node\\Interface",
    "Wow6432Node\\AppID",
    "Wow6432Node\\TypeLib",
];

/// One leftover found.
pub(in crate::windows::runner) enum Item {
    /// A class key, by its path under the classes' root.
    Class(Vec<u16>),
    /// A scheduled task, by its folder and name.
    Task { folder: ITaskFolder, name: BSTR },
    /// A BITS job.
    Bits(IBackgroundCopyJob),
    /// A value of `RUN_KEYS[key]`.
    Value { key: usize, name: Vec<u16> },
    /// An entry of the Startup folder, as its listing gave it.
    Entry {
        name: Vec<u16>,
        folder: bool,
        reparse: bool,
    },
}

/// The calls, as this token's user. Fields drop in order: COM, then Task Scheduler's module, last.
pub(in crate::windows::runner) struct OsLeftovers<'a> {
    token: &'a Handle,
    me: Sid,
    /// Its account name, for tasks that name it so; empty when it can't be read.
    account: String,
    /// The classes' root under `HKEY_CURRENT_USER`: [`CLASSES`] but in the tests.
    classes: String,
    /// The Startup folder, open while its entries are deleted.
    startup: Option<Object>,
    /// COM for this thread, once a kind reached over it is listed.
    com: Option<com::Com>,
    /// Task Scheduler's module, once tasks are listed. Declared after `com`, so it is freed
    /// after COM is uninitialised.
    library: Option<com::Library>,
}

impl<'a> OsLeftovers<'a> {
    /// The calls for `token` (this process's own, opened with `TOKEN_QUERY` and
    /// `TOKEN_IMPERSONATE` at least), whose user is `me`. Nothing is read yet.
    pub(in crate::windows::runner) fn new(token: &'a Handle, me: Sid) -> OsLeftovers<'a> {
        OsLeftovers {
            token,
            account: account(&me).unwrap_or_default(),
            me,
            classes: CLASSES.to_owned(),
            startup: None,
            com: None,
            library: None,
        }
    }

    /// The class keys that exist.
    fn classes(&self) -> io::Result<Vec<Item>> {
        let mut items = Vec::new();
        for key in CLASS_KEYS {
            let path = wide(&format!("{}\\{key}", self.classes));
            if open_key(PCWSTR(path.as_ptr()), KEY_QUERY_VALUE)?.is_some() {
                items.push(Item::Class(path));
            }
        }
        Ok(items)
    }

    /// Initialises COM for this thread, once.
    fn com(&mut self) {
        if self.com.is_none() {
            self.com = Some(com::Com::new());
        }
    }

    /// The Startup folder's entries; the folder stays open in `self.startup`. Its path is read
    /// only now, after the classes and the `Run` values are gone.
    fn startup(&mut self) -> io::Result<Vec<Item>> {
        self.startup = None;
        let profile = profile_folder(self.token)?;
        let startup = startup_path(self.token)?;
        let (profile, startup) = (&profile, &startup);
        let parts = below(startup, profile).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the Startup folder isn't in the profile",
            )
        })?;
        let mut folder = Object::open_folder(Path::new(profile))?;
        refuse_link(&folder)?;
        for part in parts {
            let name: Vec<u16> = part.encode_utf16().collect();
            folder = match folder.open_child(&name, true) {
                Ok(child) => child,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error),
            };
            refuse_link(&folder)?;
        }
        let items = folder
            .children()?
            .into_iter()
            .map(|child| Item::Entry {
                name: child.name,
                folder: child.folder,
                reparse: child.reparse,
            })
            .collect();
        self.startup = Some(folder);
        Ok(items)
    }
}

impl Leftovers for OsLeftovers<'_> {
    type Item = Item;

    fn list(&mut self, kind: Kind) -> io::Result<Vec<Item>> {
        if kind.over_com() {
            self.com();
        }
        match kind {
            Kind::Classes => self.classes(),
            Kind::Tasks => {
                if self.library.is_none() {
                    self.library = Some(com::Library::load()?);
                }
                let library = self
                    .library
                    .as_ref()
                    .ok_or_else(|| io::Error::other("taskschd.dll isn't loaded"))?;
                com::tasks(&self.me, &self.account, library)
            }
            Kind::Bits => com::bits_jobs(),
            Kind::RunKeys => run_values(),
            Kind::Startup => self.startup(),
        }
    }

    fn delete(&mut self, _: Kind, item: &Item) -> io::Result<()> {
        match item {
            Item::Class(path) => delete_tree(path),
            Item::Task { folder, name } => com::delete_task(folder, name),
            Item::Bits(job) => com::cancel_job(job),
            Item::Value { key, name } => delete_value(*key, name),
            Item::Entry {
                name,
                folder,
                reparse,
            } => {
                let dir = self
                    .startup
                    .as_ref()
                    .ok_or_else(|| io::Error::other("the Startup folder isn't open"))?;
                delete_entry(dir, name, *folder && !*reparse, 0)
            }
        }
    }
}

/// Refuses a folder opened as a link.
fn refuse_link(folder: &Object) -> io::Result<()> {
    if folder.name_surrogate() || !folder.folder {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a link on the way to the Startup folder",
        ));
    }
    Ok(())
}

/// Deletes `name` in `dir`, opened as itself; a real folder (`folder`) is emptied first.
fn delete_entry(dir: &Object, name: &[u16], folder: bool, depth: usize) -> io::Result<()> {
    if folder {
        if depth >= FOLDER_DEPTH {
            return Err(io::Error::other("folders nested too deep"));
        }
        let inside = dir.open_child(name, true)?;
        // A folder swapped for a link since the listing is deleted as the link it is now.
        if !inside.name_surrogate() && inside.folder {
            for child in inside.children()? {
                delete_entry(
                    &inside,
                    &child.name,
                    child.folder && !child.reparse,
                    depth + 1,
                )?;
            }
        }
    }
    let handle = open_at(
        dir.handle(),
        name,
        DELETE.0 | SYNCHRONIZE.0 | FILE_READ_ATTRIBUTES.0,
        true,
    )?;
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0,
        ),
    };
    let len = u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).map_err(io::Error::other)?;
    // SAFETY: the handle is open with DELETE; `info` is a live FILE_DISPOSITION_INFO_EX of
    // `len` bytes.
    unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfoEx,
            (&raw const info).cast(),
            len,
        )
    }
    .map_err(|e| os_error(&e))
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by RegOpenKeyExW and closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// Opens `HKEY_CURRENT_USER\<path>` for `access`; `None` when it doesn't exist.
fn open_key(path: PCWSTR, access: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
    let mut key = HKEY::default();
    // SAFETY: `path` is a NUL-terminated literal; `key` is a live out-pointer, owned by `Key`
    // once written.
    let code = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), access, &raw mut key) };
    match code {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => Err(win32(other)),
    }
}

/// `text`, NUL-terminated.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Deletes `HKEY_CURRENT_USER\<path>` (NUL-terminated) and all below it; one gone already is
/// fine. A registry link below it is followed with this user's rights alone.
fn delete_tree(path: &[u16]) -> io::Result<()> {
    // SAFETY: `path` is NUL-terminated and outlives the call.
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr())) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        other => Err(win32(other)),
    }
}

/// Every value name of the `Run` and `RunOnce` keys.
fn run_values() -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for (at, path) in RUN_KEYS.iter().enumerate() {
        let Some(key) = open_key(*path, KEY_QUERY_VALUE)? else {
            continue;
        };
        let mut name = vec![0u16; NAME_CAP];
        for index in 0u32.. {
            let mut len = u32::try_from(name.len()).map_err(io::Error::other)?;
            // SAFETY: the key is open; `name` is writable for `len` units.
            let code = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            match code {
                ERROR_SUCCESS => {}
                ERROR_NO_MORE_ITEMS => break,
                other => return Err(win32(other)),
            }
            let len = usize::try_from(len).map_err(io::Error::other)?;
            items.push(Item::Value {
                key: at,
                name: name.get(..len).unwrap_or_default().to_vec(),
            });
        }
    }
    Ok(items)
}

/// Deletes the value `name` of `RUN_KEYS[key]`; one gone already is fine.
fn delete_value(key: usize, name: &[u16]) -> io::Result<()> {
    let path = RUN_KEYS
        .get(key)
        .ok_or_else(|| io::Error::other("no such key"))?;
    let Some(key) = open_key(*path, KEY_SET_VALUE)? else {
        return Ok(());
    };
    let wide: Vec<u16> = name.iter().copied().chain([0]).collect();
    // SAFETY: the key is open with KEY_SET_VALUE; `wide` is NUL-terminated and outlives the call.
    match unsafe { RegDeleteValueW(key.0, PCWSTR(wide.as_ptr())) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        other => Err(win32(other)),
    }
}

/// `FOLDERID_Startup` for `token`'s user, whether or not it exists.
fn startup_path(token: &Handle) -> io::Result<String> {
    let folder = FOLDERID_Startup;
    // SAFETY: `folder` is a live GUID; the token is open with the rights the call needs. The
    // string returned is freed below.
    let path =
        unsafe { SHGetKnownFolderPath(&raw const folder, KF_FLAG_DONT_VERIFY, Some(token.0)) }
            .map_err(|e| os_error(&e))?;
    // SAFETY: the call wrote a NUL-terminated string.
    let text = unsafe { path.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by SHGetKnownFolderPath, freed once, here.
    unsafe { CoTaskMemFree(Some(path.0.cast_const().cast())) };
    text
}

/// `me`'s account name (`LookupAccountSidW`), without its domain.
fn account(me: &Sid) -> io::Result<String> {
    let sid = LocalSid::new(me)?;
    let mut name = vec![0u16; 256];
    let mut domain = vec![0u16; 256];
    let mut name_len = u32::try_from(name.len()).map_err(io::Error::other)?;
    let mut domain_len = u32::try_from(domain.len()).map_err(io::Error::other)?;
    let mut usage = SID_NAME_USE::default();
    // SAFETY: the SID is live; each buffer is writable for its length; the out-pointers are live.
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid.as_psid(),
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut usage,
        )
    }
    .map_err(|e| os_error(&e))?;
    let len = usize::try_from(name_len).map_err(io::Error::other)?;
    String::from_utf16(name.get(..len).unwrap_or_default()).map_err(io::Error::other)
}

fn win32(code: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code.0).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests;
