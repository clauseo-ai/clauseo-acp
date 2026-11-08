//! A fake [`LaunchStore`]: the value, the policy, the backup and the state in memory, every call
//! logged, and any one call made to fail.

use super::seam::{Backup, LaunchStore, Raw, StoreError};

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
    /// The call that fails, by its name.
    pub(super) fail: Option<&'static str>,
    /// Every call, by its name, in order.
    pub(super) calls: Vec<&'static str>,
}

impl FakeStore {
    /// A machine whose value is `value`, with no policy, backup or state.
    pub(super) fn new(value: Raw) -> FakeStore {
        FakeStore {
            policy: None,
            value,
            backup: None,
            state: None,
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
}
