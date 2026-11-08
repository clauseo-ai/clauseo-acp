//! The refresh's real [`Refresher`]: the fixed paths under `FOLDERID_ProgramFiles`, the record
//! in `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view), the token check, and the Event Log.
#![expect(
    unsafe_code,
    reason = "FFI to SHGetKnownFolderPath, the registry writes and the Event Log"
)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::EventLog::{
    DeregisterEventSource, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, RegisterEventSourceW,
    ReportEventW,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegFlushKey,
    RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{HSTRING, PCWSTR, w};

use super::source::{Handle, read_capped};
use super::{
    EXE_CAP, Failed, OWN_MACHINE, RefreshExit, RefreshRecord, Refresher, STAGING_SDDL, SourceError,
    SourceFile, Sources, Trust, UPDATER_KEY, pe_version, refresh, staging, walk,
};
use crate::windows::admin_copy::{self, Found, level_safe, locked_file_sddl, os};
pub(crate) use crate::windows::admin_copy::{SETUP_EXE as EXE_NAME, SETUP_SIG as SIG_NAME};
use crate::windows::detect::native_machine;
use crate::windows::record::read_string;
use crate::windows::setup::adopt::windows_adopter::own_profile;
use crate::windows::setup::lock::{Held, SETUP_MUTEX, try_lock};
use crate::windows::setup::owner::this_process;
use crate::windows::setup::record_values::{
    ADMIN_COPY_OWNER, ADMIN_COPY_SHA256, ADMIN_COPY_VERSION, GROUP_SID, USER_INSTALL_DIR,
};
use crate::windows::sids::Sid;

/// The record's key under `HKLM`.
const RECORD_KEY: &str = "SOFTWARE\\Clauseo\\Sandbox";
/// The staged copy's name in staging.
pub(super) const STAGED_NAME: &str = "clauseo-sandbox-setup.exe.new";
/// The set-aside copy's name in staging.
pub(super) const SET_ASIDE_NAME: &str = "clauseo-sandbox-setup.exe.old";
/// The Event Log source the refresh writes as (the Application log).
const EVENT_SOURCE: PCWSTR = w!("Clauseo Sandbox");

/// `--refresh`: [`refresh`] over a [`WindowsRefresher`] with the key and version built in.
pub fn run() -> RefreshExit {
    let mut fs = WindowsRefresher::new();
    let trust = Trust {
        key: UPDATER_KEY,
        machine: OWN_MACHINE,
    };
    refresh(&mut fs, &trust)
}

/// The real machine. Its paths are found once, from `FOLDERID_ProgramFiles`.
pub struct WindowsRefresher {
    /// `<ProgramFiles>\Clauseo\Sandbox`, or why it couldn't be found.
    sandbox: Result<PathBuf, Failed>,
    /// The admin folder, held through the renames (`hold_admin_folder`).
    held: Option<Handle>,
    /// The set-up lock, held through the run.
    lock: Option<Held>,
}

impl Default for WindowsRefresher {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsRefresher {
    /// The machine, its folder found from `FOLDERID_ProgramFiles`.
    pub fn new() -> WindowsRefresher {
        WindowsRefresher {
            sandbox: sandbox_dir(),
            held: None,
            lock: None,
        }
    }

    /// `<sandbox>\<rel>`.
    fn at(&self, rel: &str) -> Result<PathBuf, Failed> {
        self.sandbox
            .as_ref()
            .map(|dir| dir.join(rel))
            .map_err(Clone::clone)
    }

    /// `<sandbox>\staging`.
    fn staging(&self) -> Result<PathBuf, Failed> {
        self.at(admin_copy::STAGING)
    }

    /// `<sandbox>\staging\<name>`.
    fn staged(&self, name: &str) -> Result<PathBuf, Failed> {
        Ok(self.staging()?.join(name))
    }
}

/// `<FOLDERID_ProgramFiles>\Clauseo\Sandbox`: where the admin copy and staging live.
pub(crate) fn sandbox_dir() -> Result<PathBuf, Failed> {
    Ok(admin_copy::admin_dir(&program_files()?))
}

/// `%ProgramFiles%` as Windows knows it (`FOLDERID_ProgramFiles`), never from the environment.
fn program_files() -> Result<PathBuf, Failed> {
    let folder = FOLDERID_ProgramFiles;
    // SAFETY: `folder` is a live GUID for the call; no token is passed. The string returned is
    // copied, then freed with CoTaskMemFree, once.
    let path = unsafe { SHGetKnownFolderPath(&raw const folder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| format!("SHGetKnownFolderPath failed ({})", error.code().0))?;
    // SAFETY: on success the call wrote a NUL-terminated string at `path`.
    let units = unsafe { path.as_wide() }.to_vec();
    // SAFETY: the string came from SHGetKnownFolderPath and is freed once, here.
    unsafe { CoTaskMemFree(Some(path.0.cast_const().cast())) };
    if units.is_empty() {
        return Err("Windows gave no Program Files folder".to_owned());
    }
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

impl Refresher for WindowsRefresher {
    fn is_system(&mut self) -> Result<bool, Failed> {
        this_process()
            .map(|(_elevated, system)| system)
            .map_err(|error| format!("this process's token can't be read: {error}"))
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        std::env::current_exe().map_err(|error| format!("this exe's path: {error}"))
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        self.at(EXE_NAME)
    }

    fn lock(&mut self) -> Result<bool, Failed> {
        self.lock = try_lock(SETUP_MUTEX).map_err(|error| format!("the set-up lock: {error}"))?;
        Ok(self.lock.is_some())
    }

    fn unlock(&mut self) {
        self.lock = None;
    }

    fn record(&mut self) -> Result<RefreshRecord, Failed> {
        let key = HSTRING::from(RECORD_KEY);
        let read = |value: &str| {
            let value = HSTRING::from(value);
            read_string(PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()))
                .map_err(|error| format!("the record: {error}"))
        };
        Ok(RefreshRecord {
            copy_owner: read(ADMIN_COPY_OWNER)?,
            user_install_dir: read(USER_INSTALL_DIR)?,
            admin_copy_sha256: read(ADMIN_COPY_SHA256)?,
            admin_copy_version: read(ADMIN_COPY_VERSION)?,
            group_sid: read(GROUP_SID)?,
        })
    }

    fn profile_dir(&mut self, owner: &Sid) -> Result<Option<PathBuf>, Failed> {
        // A temporary profile isn't the owner's own folder: none, so the refresh refuses.
        own_profile(owner).map_err(|error| format!("the owner's profile: {error}"))
    }

    fn admin_only_dir(&mut self, dir: &Path) -> Result<bool, Failed> {
        let what = |error: std::io::Error| format!("the install folder: {error}");
        if os::found(dir).map_err(what)? != Found::Folder {
            return Ok(false);
        }
        Ok(level_safe(&os::security(dir).map_err(what)?))
    }

    fn clear_leftovers(&mut self) -> Result<(), Failed> {
        staging::delete(&self.staged(SET_ASIDE_NAME)?)?;
        staging::delete(&self.staged(STAGED_NAME)?)
    }

    fn read_sources(&mut self, dir: &Path) -> Result<Sources, (SourceFile, SourceError)> {
        walk::read_sources(dir)
    }

    fn stage(&mut self, exe: &[u8]) -> Result<Vec<u8>, Failed> {
        staging::secure_dir(&self.staging()?, STAGING_SDDL)?;
        let staged = self.staged(STAGED_NAME)?;
        staging::write_new(&staged, exe, STAGING_SDDL)?;
        read_capped(&staged, EXE_CAP).map_err(|error| format!("the staged copy: {error}"))
    }

    fn admin_copy_sha256(&mut self) -> Result<Option<[u8; 32]>, Failed> {
        match read_capped(&self.at(EXE_NAME)?, EXE_CAP) {
            Ok(bytes) => Ok(Some(Sha256::digest(&bytes).into())),
            Err(SourceError::Missing) => Ok(None),
            Err(error) => Err(format!("the admin copy: {error}")),
        }
    }

    fn admin_copy_machine(&mut self) -> Result<Option<u16>, Failed> {
        match read_capped(&self.at(EXE_NAME)?, EXE_CAP) {
            Ok(bytes) => Ok(pe_version::plain_machine(&bytes)),
            Err(SourceError::Missing) => Ok(None),
            Err(error) => Err(format!("the admin copy: {error}")),
        }
    }

    fn native_machine(&mut self) -> Result<u16, Failed> {
        native_machine()
            .map(|machine| machine.0)
            .map_err(|error| format!("IsWow64Process2 failed ({})", error.code().0))
    }

    /// The descriptor `--adopt` gives the admin copy, the record's group denied, so a refresh
    /// changes no access and the launcher's deny check (`admin_copy::group_denied`) still passes.
    fn secure_staged(&mut self, group: Option<&Sid>) -> Result<(), Failed> {
        staging::secure_file(&self.staged(STAGED_NAME)?, &locked_file_sddl(group))
    }

    fn deny_group(&mut self, group: &Sid) -> Result<bool, Failed> {
        let sandbox = self.sandbox.clone()?;
        os::heal_deny(&sandbox, &sandbox.join(EXE_NAME), group)
            .map_err(|error| format!("the admin folder's deny: {error}"))
    }

    fn hold_admin_folder(&mut self) -> Result<(), Failed> {
        let sandbox = self.sandbox.clone()?;
        self.held = Some(staging::hold_dir(&sandbox)?);
        Ok(())
    }

    fn release_admin_folder(&mut self) {
        self.held = None;
    }

    fn set_aside_admin_copy(&mut self) -> Result<(), Failed> {
        staging::rename(&self.at(EXE_NAME)?, &self.staged(SET_ASIDE_NAME)?)
    }

    fn place_staged(&mut self) -> Result<(), Failed> {
        staging::rename(&self.staged(STAGED_NAME)?, &self.at(EXE_NAME)?)
    }

    fn restore_admin_copy(&mut self) -> Result<(), Failed> {
        staging::rename(&self.staged(SET_ASIDE_NAME)?, &self.at(EXE_NAME)?)
    }

    fn write_version(&mut self, version: &str) -> Result<(), Failed> {
        write_record_string(ADMIN_COPY_VERSION, version)
    }

    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed> {
        write_record_string(ADMIN_COPY_SHA256, hex)
    }

    fn discard_staged(&mut self) {
        if let Ok(staged) = self.staged(STAGED_NAME) {
            let _left_for_the_next_run = staging::delete(&staged);
        }
    }

    /// After a swap the set-aside copy is this running image: deleted if Windows allows it,
    /// else scheduled for deletion at restart, and the folder after it the same way.
    fn leave_staging(&mut self, swapped: bool) {
        let Ok(dir) = self.staging() else {
            return;
        };
        let left = if swapped {
            let old = dir.join(SET_ASIDE_NAME);
            staging::delete(&old)
                .or_else(|_running| staging::delete_at_restart(&old))
                .and_then(|()| {
                    staging::remove_dir(&dir).or_else(|_held| staging::delete_at_restart(&dir))
                })
        } else {
            staging::remove_dir(&dir)
        };
        if let Err(why) = left {
            event_log(true, &format!("the refresh's staging folder stays: {why}"));
        }
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}

/// Sets the `REG_SZ` `name` of the record's key (which must exist) to `text`, and flushes the
/// key, so the value is on disk before the run ends.
fn write_record_string(name: &str, text: &str) -> Result<(), Failed> {
    let subkey = HSTRING::from(RECORD_KEY);
    let mut key = HKEY::default();
    // SAFETY: the subkey is NUL-terminated and outlives the call; `key` is a live out-pointer,
    // closed below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return Err(format!("RegOpenKeyExW failed ({})", opened.0));
    }
    let data: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let name = HSTRING::from(name);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data` a live
    // slice, a NUL-terminated UTF-16 string.
    let set = unsafe { RegSetValueExW(key, &name, None, REG_SZ, Some(&data)) };
    // SAFETY: the key is open.
    let flushed = unsafe { RegFlushKey(key) };
    // SAFETY: the key came from RegOpenKeyExW and is closed once, here.
    let _closed = unsafe { RegCloseKey(key) };
    if set != ERROR_SUCCESS {
        return Err(format!("RegSetValueExW failed ({})", set.0));
    }
    if flushed != ERROR_SUCCESS {
        return Err(format!("RegFlushKey failed ({})", flushed.0));
    }
    Ok(())
}

/// Writes `line` to the Application log as source "Clauseo Sandbox", event 1; an error when
/// `error`. A log that can't be written is dropped: the exit code still says what happened.
/// The stop task (`setup::stop`) logs through it too.
pub fn event_log(error: bool, line: &str) {
    // SAFETY: the source name is a static NUL-terminated string; the handle, when given, is
    // deregistered below.
    let Ok(source) = (unsafe { RegisterEventSourceW(PCWSTR::null(), EVENT_SOURCE) }) else {
        return;
    };
    let text = HSTRING::from(line);
    let strings = [PCWSTR(text.as_ptr())];
    let kind = if error {
        EVENTLOG_ERROR_TYPE
    } else {
        EVENTLOG_INFORMATION_TYPE
    };
    // SAFETY: the handle is registered; `strings` holds one NUL-terminated string that
    // outlives the call; no SID or raw data.
    let _reported = unsafe { ReportEventW(source, kind, 0, 1, None, 0, Some(&strings), None) };
    // SAFETY: the handle came from RegisterEventSourceW and is deregistered once, here.
    let _deregistered = unsafe { DeregisterEventSource(HANDLE(source.0)) };
}
