//! The real [`WslWatch`](super::WslWatch) pieces (`schedule.rs`): the change notification on
//! `HKLM\SOFTWARE\Classes\AppID` and on Windows Installer's keys, `InProgress`, and the
//! `_MSIExecute` hint. Machine keys in the 64-bit view, opened from `HKEY_LOCAL_MACHINE`, never
//! through `HKEY_CLASSES_ROOT` (which would merge in a user's classes).
#![expect(
    unsafe_code,
    reason = "FFI to the registry notification, the events and Windows Installer's mutex"
)]

use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_NOTIFY, KEY_WOW64_64KEY, REG_NOTIFY_CHANGE_LAST_SET,
    REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_CHANGE_SECURITY, REG_NOTIFY_THREAD_AGNOSTIC, RRF_RT_ANY,
    RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, OpenMutexW, ResetEvent, SYNCHRONIZATION_SYNCHRONIZE, WaitForMultipleObjects,
};
use windows::core::{HSTRING, PCWSTR, Param, w};

use super::super::Failed;
use super::Installer;
use crate::windows::acl::object::Handle;

/// WSL's `AppID` keys' parent, watched with its subtree.
const APP_ID: PCWSTR = w!(r"SOFTWARE\Classes\AppID");
/// Where Windows Installer writes `InProgress` while an install runs, as a key or a value:
/// each parent is read and watched (its own names and values, not its subtree). The ADR's VM
/// row records which one an install writes.
const INSTALLER: [&str; 2] = [
    r"SOFTWARE\Microsoft\Windows\Installer",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer",
];
/// `InProgress`'s name, under each of [`INSTALLER`].
const IN_PROGRESS: &str = "InProgress";

/// The run's clock: milliseconds since boot, which `Instant` would read too.
pub(in crate::windows::refresh) fn now() -> Duration {
    // SAFETY: GetTickCount64 only reads the tick count.
    Duration::from_millis(unsafe { GetTickCount64() })
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// One watched key: its event and whether a notification on it is armed and unfired.
struct Watched {
    key: Key,
    subtree: bool,
    event: Handle,
    armed: bool,
}

/// The notifications on [`APP_ID`] (its subtree) and on each of [`INSTALLER`] that exists.
pub(in crate::windows::refresh) struct Watch {
    keys: Vec<Watched>,
}

impl Watch {
    /// Opens the keys, for notification only. [`APP_ID`] must open; an installer key that
    /// isn't there is read each time instead.
    pub(in crate::windows::refresh) fn open() -> Result<Watch, Failed> {
        let app_id = open(APP_ID)
            .map_err(|code| format!("RegOpenKeyExW on the AppID key failed ({code})"))?;
        let mut keys = vec![watched(app_id, true)?];
        for path in INSTALLER {
            if let Ok(key) = open(&HSTRING::from(path)) {
                keys.push(watched(key, false)?);
            }
        }
        Ok(Watch { keys })
    }

    /// Arms a notification on each key with none armed: the event reset, then
    /// `RegNotifyChangeKeyValue` (names, values and descriptors, thread agnostic, so it isn't
    /// tied to the thread's life). One armed and unfired is left as it is: it fires on the next
    /// change.
    pub(in crate::windows::refresh) fn arm(&mut self) -> Result<(), Failed> {
        let filter = REG_NOTIFY_CHANGE_NAME
            | REG_NOTIFY_CHANGE_LAST_SET
            | REG_NOTIFY_CHANGE_SECURITY
            | REG_NOTIFY_THREAD_AGNOSTIC;
        for watched in self.keys.iter_mut().filter(|watched| !watched.armed) {
            // SAFETY: the event is a live manual-reset event this watch owns.
            unsafe { ResetEvent(watched.event.0) }
                .map_err(|error| format!("ResetEvent failed ({})", error.code().0))?;
            // SAFETY: the key is open with KEY_NOTIFY and the event live; both outlive the
            // notification, which closing the key ends.
            let code = unsafe {
                RegNotifyChangeKeyValue(
                    watched.key.0,
                    watched.subtree,
                    filter,
                    Some(watched.event.0),
                    true,
                )
            };
            if code != ERROR_SUCCESS {
                return Err(format!("RegNotifyChangeKeyValue failed ({})", code.0));
            }
            watched.armed = true;
        }
        Ok(())
    }

    /// Waits up to `time` for any armed notification: `true` when one fired, `false` when the
    /// time ran out. A wait that fails is the error, so the caller waits without the watch
    /// rather than spin on a wait that returns at once.
    pub(in crate::windows::refresh) fn wait(&mut self, time: Duration) -> Result<bool, Failed> {
        let events: Vec<HANDLE> = self.keys.iter().map(|watched| watched.event.0).collect();
        let ms = u32::try_from(time.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: every event is live; the slice outlives the call.
        let woke = unsafe { WaitForMultipleObjects(&events, false, ms) };
        if woke == WAIT_TIMEOUT {
            return Ok(false);
        }
        let fired = woke.0.wrapping_sub(WAIT_OBJECT_0.0);
        let Some(watched) = usize::try_from(fired)
            .ok()
            .and_then(|at| self.keys.get_mut(at))
        else {
            return Err(format!("WaitForMultipleObjects gave {:#x}", woke.0));
        };
        watched.armed = false;
        Ok(true)
    }
}

/// `path` under `HKEY_LOCAL_MACHINE`, 64-bit view, for notification.
fn open(path: impl Param<PCWSTR>) -> Result<Key, u32> {
    let mut key = HKEY::default();
    // SAFETY: `path` is NUL-terminated and outlives the call; `key` is a live out-pointer.
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path,
            None,
            KEY_NOTIFY | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    if code == ERROR_SUCCESS {
        Ok(Key(key))
    } else {
        Err(code.0)
    }
}

/// `key` with a manual-reset event of its own, nothing armed yet.
fn watched(key: Key, subtree: bool) -> Result<Watched, Failed> {
    // SAFETY: no attributes, no name: a fresh event this watch owns.
    let event = unsafe { CreateEventW(None, true, false, None) }
        .map_err(|error| format!("CreateEventW failed ({})", error.code().0))?;
    Ok(Watched {
        key,
        subtree,
        event: Handle(event),
        armed: false,
    })
}

/// Windows Installer's state: `InProgress` as a key or a value under any of [`INSTALLER`], and
/// the `_MSIExecute` hint. A read that fails but for "not there" counts as in progress: the run
/// holds, bounded by its budget, rather than end early.
pub(in crate::windows::refresh) fn installer() -> Installer {
    let name = HSTRING::from(IN_PROGRESS);
    let in_progress = INSTALLER.into_iter().any(|path| {
        let parent = HSTRING::from(path);
        // SAFETY: both names are NUL-terminated and outlive the call; no buffer is asked for,
        // so the call only says whether `InProgress` is there as a value.
        let value = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &parent,
                &name,
                RRF_RT_ANY | RRF_SUBKEY_WOW6464KEY,
                None,
                None,
                None,
            )
        };
        // As a key: opened, then closed at once.
        let key = open(&HSTRING::from(format!(r"{path}\{IN_PROGRESS}"))).map(drop);
        there(value) || key.map_or_else(|code| there(WIN32_ERROR(code)), |()| true)
    });
    Installer {
        in_progress,
        mutex: mutex_held(),
    }
}

/// Whether a registry read's `code` says the name is there; "not there" is the one no.
fn there(code: WIN32_ERROR) -> bool {
    code != ERROR_FILE_NOT_FOUND && code != ERROR_PATH_NOT_FOUND
}

/// Whether `Global\_MSIExecute` is there: opened for `SYNCHRONIZE` only, or refused, which
/// means it exists. A hint: anyone with the right to make a global object can hold it.
fn mutex_held() -> bool {
    // SAFETY: the name is a NUL-terminated constant; the handle is closed on drop.
    match unsafe {
        OpenMutexW(
            SYNCHRONIZATION_SYNCHRONIZE,
            false,
            w!(r"Global\_MSIExecute"),
        )
    } {
        Ok(handle) => {
            drop(Handle(handle));
            true
        }
        Err(error) => {
            let code = WIN32_ERROR::from_error(&error);
            code.is_some_and(|code| code != ERROR_FILE_NOT_FOUND)
        }
    }
}
