//! The real [`Record`](super::Record)'s effects, which each pool task's machine calls: the
//! set-up lock (waited for up to [`LOCK_WAIT_MS`]), the record's `REG_QWORD` values (and the
//! sweep's one `REG_DWORD`) in
//! `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view, written without making the key), and the wall
//! clock.
#![expect(unsafe_code, reason = "GetSystemTimeAsFileTime")]

use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

use super::Failed;
use crate::windows::setup::lock::{Held, SETUP_MUTEX, SetupMutex};
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::record_write::{self, RECORD_KEY};

/// How long a pool task waits for the set-up lock (to reserve its number, mark users, or write
/// its completion), in milliseconds: well inside the task's 2-minute limit.
pub(crate) const LOCK_WAIT_MS: u32 = 30_000;

/// The set-up lock, while a pool task holds it.
#[derive(Default)]
pub(crate) struct RecordLock {
    held: Option<Held>,
}

impl RecordLock {
    /// Waits up to [`LOCK_WAIT_MS`] for the set-up lock and keeps it: `Ok(false)` when another
    /// still holds it.
    pub(crate) fn lock(&mut self) -> Result<bool, Failed> {
        let mutex = SetupMutex::open(SETUP_MUTEX).map_err(|error| error.to_string())?;
        match mutex.acquire(LOCK_WAIT_MS) {
            Ok((held, _acquired)) => {
                self.held = Some(held);
                Ok(true)
            }
            Err(MachineError::LockHeld) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Gives the lock back, if held.
    pub(crate) fn unlock(&mut self) {
        self.held = None;
    }
}

/// The record's `REG_QWORD` value `name`: `None` when it, or the key, is missing.
pub(crate) fn read_value(name: &str) -> Result<Option<u64>, Failed> {
    record_write::read_pool_value(RECORD_KEY, name).map_err(|error| error.to_string())
}

/// Sets the record's `REG_QWORD` value `name` in the key as it is, never making it.
pub(crate) fn write_value(name: &str, value: u64) -> Result<(), Failed> {
    record_write::write_pool_value(RECORD_KEY, name, value).map_err(|error| error.to_string())
}

/// Sets the record's `REG_DWORD` value `name` in the key as it is, never making it.
pub(crate) fn write_dword(name: &str, value: u32) -> Result<(), Failed> {
    record_write::write_pool_dword(RECORD_KEY, name, value).map_err(|error| error.to_string())
}

/// The wall clock, as a `FILETIME` (100 ns since 1601, UTC).
pub(crate) fn now() -> u64 {
    // SAFETY: GetSystemTimeAsFileTime only reads the clock.
    let now = unsafe { GetSystemTimeAsFileTime() };
    u64::from(now.dwHighDateTime) << 32 | u64::from(now.dwLowDateTime)
}
