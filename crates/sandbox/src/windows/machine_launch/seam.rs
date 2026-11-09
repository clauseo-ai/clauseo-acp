//! What the launch restriction's guard asks of the machine: the value, the Group Policy value
//! that overrides it, `MachineAccessRestriction` beside it and the access descriptor COM uses,
//! `MachineLaunchRestrictionOld` (where DCOM moves a value it won't keep), Clauseo's backup of
//! the value, and the guard's state in the record. `os.rs` is the registry and COM; the tests
//! fake them.

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
    /// The `MachineAccessRestriction` bytes Clauseo wrote where there was none (COM's own
    /// effective access descriptor, so nothing changes in effect); `None` when it wrote none.
    /// [`super::remove`] deletes the value only while it is still exactly these bytes.
    pub access: Option<Vec<u8>>,
    /// Every value Clauseo wrote, oldest first, at most [`super::hold::HISTORY_MAX`], the
    /// last one `written`: the removal takes any of them out of `MachineLaunchRestrictionOld`.
    pub history: Vec<Vec<u8>>,
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
    /// `HKLM\SOFTWARE\Microsoft\Ole\MachineAccessRestriction`. DCOM keeps the launch value
    /// only while this one is there too: without it, it moves a new launch value to
    /// `MachineLaunchRestrictionOld` within milliseconds.
    fn access(&mut self) -> Result<Raw, StoreError>;
    /// Whether Group Policy's
    /// `HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DCOM\MachineAccessRestriction` is set (the
    /// "DCOM: Machine Access Restrictions in SDDL syntax" policy), in any type.
    fn access_policy(&mut self) -> Result<bool, StoreError>;
    /// The machine access descriptor COM uses now, self-relative
    /// (`CoGetSystemSecurityPermissions(SD_ACCESSRESTRICTIONS)`): Windows' built-in one when
    /// there is no value.
    fn effective_access(&mut self) -> Result<Vec<u8>, StoreError>;
    /// Writes `bytes` as the `REG_BINARY` `MachineAccessRestriction`.
    fn write_access(&mut self, bytes: &[u8]) -> Result<(), StoreError>;
    /// Deletes `MachineAccessRestriction`; one already gone is `Ok`.
    fn delete_access(&mut self) -> Result<(), StoreError>;
    /// `HKLM\SOFTWARE\Microsoft\Ole\MachineLaunchRestrictionOld`.
    fn old(&mut self) -> Result<Raw, StoreError>;
    /// Deletes `MachineLaunchRestrictionOld`; one already gone is `Ok`.
    fn delete_old(&mut self) -> Result<(), StoreError>;
    /// Waits `millis` milliseconds, for DCOM to act on a write.
    fn settle(&mut self, millis: u64);
}
