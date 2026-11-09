//! A fake [`LaunchStore`]: the value, the policy, the backup, the state, the access value,
//! COM's effective access descriptor and `MachineLaunchRestrictionOld` in memory, every call
//! logged, and any one call made to fail. A settle acts as DCOM does: with no access value (or
//! [`FakeStore::moves`] set), it moves the launch value to `…Old`.

use super::seam::{Backup, LaunchStore, Raw, StoreError};
use super::test_sd::{windows_11, windows_11_aces};

/// The machine as the guard sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FakeStore {
    /// Group Policy's value, converted: `None` when it isn't set.
    pub(super) policy: Option<Result<Vec<u8>, String>>,
    /// `MachineLaunchRestriction`.
    pub(super) value: Raw,
    /// Clauseo's backup.
    pub(super) backup: Option<Backup>,
    /// The record's `MachineLaunchDeny`.
    pub(super) state: Option<String>,
    /// `MachineAccessRestriction`; present by default.
    pub(super) access: Raw,
    /// Whether Group Policy's `MachineAccessRestriction` is set.
    pub(super) access_policy: bool,
    /// What COM gives as its effective machine launch descriptor.
    pub(super) effective_launch: Result<Vec<u8>, String>,
    /// Whether DCOM rewrites both values at each settle ([`dcom_rewrite`]).
    pub(super) rewrites: bool,
    /// What COM gives as its effective machine access descriptor.
    pub(super) effective: Result<Vec<u8>, String>,
    /// `MachineLaunchRestrictionOld`.
    pub(super) old: Raw,
    /// Whether DCOM moves the launch value at each settle even beside an access value.
    pub(super) moves: bool,
    /// The call that fails, by its name.
    pub(super) fail: Option<&'static str>,
    /// Every call, by its name, in order.
    pub(super) calls: Vec<&'static str>,
}

impl FakeStore {
    /// A machine whose value is `value`, with no policy, backup or state, and an access value.
    pub(super) fn new(value: Raw) -> FakeStore {
        FakeStore {
            policy: None,
            value,
            backup: None,
            state: None,
            access: Raw::Binary(machine_access()),
            access_policy: false,
            effective_launch: Ok(windows_11()),
            rewrites: false,
            effective: Ok(machine_access()),
            old: Raw::Absent,
            moves: false,
            fail: None,
            calls: Vec::new(),
        }
    }

    /// Logs `name`, and fails it when it is the one to fail.
    fn call(&mut self, name: &'static str) -> Result<(), StoreError> {
        self.calls.push(name);
        if self.fail == Some(name) {
            return Err(format!("{name} failed (5)"));
        }
        Ok(())
    }

    /// The calls that changed something.
    pub(super) fn writes(&self) -> Vec<&'static str> {
        let changing = |call: &&&str| call.starts_with("write") || call.starts_with("delete");
        self.calls.iter().filter(changing).copied().collect()
    }
}

impl LaunchStore for FakeStore {
    fn policy(&mut self) -> Result<Option<Result<Vec<u8>, String>>, StoreError> {
        self.call("policy")?;
        Ok(self.policy.clone())
    }

    fn read(&mut self) -> Result<Raw, StoreError> {
        self.call("read")?;
        Ok(self.value.clone())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.call("write")?;
        self.value = Raw::Binary(bytes.to_vec());
        Ok(())
    }

    fn delete(&mut self) -> Result<(), StoreError> {
        self.call("delete")?;
        self.value = Raw::Absent;
        Ok(())
    }

    fn backup(&mut self) -> Result<Option<Backup>, StoreError> {
        self.call("backup")?;
        Ok(self.backup.clone())
    }

    fn write_backup(&mut self, backup: &Backup) -> Result<(), StoreError> {
        self.call("write_backup")?;
        self.backup = Some(backup.clone());
        Ok(())
    }

    fn delete_backup(&mut self) -> Result<(), StoreError> {
        self.call("delete_backup")?;
        self.backup = None;
        Ok(())
    }

    fn state(&mut self) -> Result<Option<String>, StoreError> {
        self.call("state")?;
        Ok(self.state.clone())
    }

    fn write_state(&mut self, state: &str) -> Result<(), StoreError> {
        self.call("write_state")?;
        self.state = Some(state.to_owned());
        Ok(())
    }

    fn delete_state(&mut self) -> Result<(), StoreError> {
        self.call("delete_state")?;
        self.state = None;
        Ok(())
    }

    fn access(&mut self) -> Result<Raw, StoreError> {
        self.call("access")?;
        Ok(self.access.clone())
    }

    fn access_policy(&mut self) -> Result<bool, StoreError> {
        self.call("access_policy")?;
        Ok(self.access_policy)
    }

    fn effective_access(&mut self) -> Result<Vec<u8>, StoreError> {
        self.call("effective_access")?;
        self.effective.clone()
    }

    fn effective_launch(&mut self) -> Result<Vec<u8>, StoreError> {
        self.call("effective_launch")?;
        self.effective_launch.clone()
    }

    fn write_access(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.call("write_access")?;
        self.access = Raw::Binary(bytes.to_vec());
        Ok(())
    }

    fn delete_access(&mut self) -> Result<(), StoreError> {
        self.call("delete_access")?;
        self.access = Raw::Absent;
        Ok(())
    }

    fn old(&mut self) -> Result<Raw, StoreError> {
        self.call("old")?;
        Ok(self.old.clone())
    }

    fn delete_old(&mut self) -> Result<(), StoreError> {
        self.call("delete_old")?;
        self.old = Raw::Absent;
        Ok(())
    }

    fn settle(&mut self, _millis: u64) {
        self.calls.push("settle");
        let dcom_moves = self.moves || self.access == Raw::Absent;
        if dcom_moves && matches!(self.value, Raw::Binary(_)) {
            self.old = std::mem::replace(&mut self.value, Raw::Absent);
        }
        if self.rewrites {
            if let Raw::Binary(bytes) = &mut self.value {
                *bytes = dcom_rewrite(bytes, &windows_11_aces());
            }
            if let Raw::Binary(bytes) = &mut self.access {
                *bytes = dcom_rewrite(bytes, &[]);
            }
        }
    }
}

/// A machine access descriptor, as COM gives it: `O:BAG:BAD:(A;;CCDCLC;;;WD)`, in COM's
/// rights.
pub(super) fn machine_access() -> Vec<u8> {
    super::test_sd::descriptor(
        &[super::test_sd::ace(super::test_sd::A, 0, 0x7, "S-1-1-0")],
        false,
    )
}

/// `bytes` as DCOM rewrites them: `defaults` merged in (by meaning), the allows reordered
/// (denies kept first), four bytes of padding after every entry, and the DACL laid out first.
pub(super) fn dcom_rewrite(bytes: &[u8], defaults: &[Vec<u8>]) -> Vec<u8> {
    let parsed = super::LaunchSd::parse(bytes).unwrap();
    let (mut denies, mut allows): (Vec<Vec<u8>>, Vec<Vec<u8>>) = parsed
        .aces()
        .iter()
        .cloned()
        .partition(|entry| super::sd::is_deny(entry));
    let key = |entry: &[u8]| {
        let sid = super::sd::sid(entry).map(<[u8]>::to_vec);
        (entry[0], entry[1], entry[4..8].to_vec(), sid)
    };
    for entry in defaults {
        if !allows.iter().any(|allow| key(allow) == key(entry)) {
            allows.push(entry.clone());
        }
    }
    allows.reverse();
    denies.extend(allows);
    let padded: Vec<Vec<u8>> = denies.iter().map(|entry| pad(entry)).collect();
    super::test_sd::descriptor(&padded, true)
}

/// `entry` with four zero bytes after it, its size saying so.
pub(super) fn pad(entry: &[u8]) -> Vec<u8> {
    let mut out = entry.to_vec();
    out.extend_from_slice(&[0; 4]);
    let size = u16::try_from(out.len()).unwrap().to_le_bytes();
    out[2..4].copy_from_slice(&size);
    out
}
