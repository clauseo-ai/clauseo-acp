//! What the launch restriction's guard asks of the machine: the value, the Group Policy value
//! that overrides it, Clauseo's backup of the value, and the guard's state in the record.
//! `os.rs` is the registry; the tests fake it.

/// `HKLM\SOFTWARE\Microsoft\Ole\MachineLaunchRestriction` as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    /// No value: Windows' default applies.
    Absent,
    /// A `REG_BINARY` value's bytes.
    Binary(Vec<u8>),
    /// A value of another type, which COM can't use and the guard never touches.
    OtherType,
}

/// Clauseo's backup of the value, written before its first change (`os.rs` keeps it in a
/// subkey of the record only SYSTEM and Administrators can open).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// The value as it was before Clauseo's first change; `None` when there was none.
    pub original: Option<Vec<u8>>,
    /// The bytes Clauseo wrote last, recorded just before each write.
    pub written: Vec<u8>,
    /// Whether a run has put the deny back since the first write (drift): the value then
    /// holds what someone else wrote, which [`super::remove`] must keep.
    pub put_back: bool,
}

/// A failed registry call, in words naming the call and its Windows error code; never a path
/// under a user's profile or a SID.
pub type StoreError = String;

/// The registry the guard reads and writes, elevated (the set-up) or as SYSTEM (the refresh).
pub trait LaunchStore {
    /// `HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DCOM\MachineLaunchRestriction`, Group
    /// Policy's SDDL, which COM uses in place of the value: `None` when it isn't set,
    /// `Some(Ok)` with the self-relative descriptor it converts to, `Some(Err)` when it
    /// doesn't convert (or isn't text).
    fn policy(&mut self) -> Result<Option<Result<Vec<u8>, String>>, StoreError>;
    /// The value.
    fn read(&mut self) -> Result<Raw, StoreError>;
    /// Writes `bytes` as the `REG_BINARY` value.
    fn write(&mut self, bytes: &[u8]) -> Result<(), StoreError>;
    /// Deletes the value; one already gone is `Ok`.
    fn delete(&mut self) -> Result<(), StoreError>;
    /// Clauseo's backup; `None` when there is none.
    fn backup(&mut self) -> Result<Option<Backup>, StoreError>;
    /// Writes the backup, replacing what was there.
    fn write_backup(&mut self, backup: &Backup) -> Result<(), StoreError>;
    /// Deletes the backup; one already gone is `Ok`.
    fn delete_backup(&mut self) -> Result<(), StoreError>;
    /// The guard's state as the record holds it (`MachineLaunchDeny`); `None` when missing.
    fn state(&mut self) -> Result<Option<String>, StoreError>;
    /// Writes the guard's state to the record, which must exist already.
    fn write_state(&mut self, state: &str) -> Result<(), StoreError>;
    /// Deletes the guard's state; one already gone is `Ok`.
    fn delete_state(&mut self) -> Result<(), StoreError>;
}
