//! The WSL seams faked in memory: a machine with WSL's classes, `AppID`s and values, the
//! machine record's two values, and each write recorded as a line.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;
use crate::windows::sids::Sid;

/// `LxssUserSession`'s `AppID`, the VM's.
pub(super) const LXSS_APP: &str = "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}";
/// `WSLCSessionManager`'s `AppID`, the VM's.
pub(super) const WSLC_APP: &str = "{E9B79997-57E3-4201-AECC-6A464E530DD2}";
/// An inbox `AppID`, made up.
pub(super) const INBOX_APP: &str = "{11111111-2222-3333-4444-555555555555}";

/// `id` as an `AppID`.
pub(super) fn app(id: &str) -> AppId {
    AppId::parse(id).unwrap()
}

/// The pool group.
pub(super) fn group() -> Sid {
    Sid::parse("S-1-5-21-7-8-9-1009").unwrap()
}

/// The stock MSI value, 120 bytes: `O:BAG:BAD:(A;;CCDCSW;;;AU)(A;;CCDCSW;;;PS)(A;;CCDCSW;;;SY)`.
pub(super) fn stock() -> Vec<u8> {
    let mut out = vec![
        1, 0, 4, 0x80, 0x14, 0, 0, 0, 0x24, 0, 0, 0, 0, 0, 0, 0, 0x34, 0, 0, 0,
    ];
    let ba = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];
    out.extend_from_slice(&ba);
    out.extend_from_slice(&ba);
    out.extend_from_slice(&[2, 0, 0x44, 0, 3, 0, 0, 0]);
    for rid in [11, 10, 18] {
        out.extend_from_slice(&[
            0, 0, 0x14, 0, 0xB, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 5, rid, 0, 0, 0,
        ]);
    }
    out
}

/// One `AppID`'s key: its `LocalService` and its `LaunchPermission`.
#[derive(Debug, Clone, Default)]
pub(super) struct Key {
    pub(super) service: Option<&'static str>,
    pub(super) value: Option<Vec<u8>>,
}

/// The machine.
#[derive(Debug, Default)]
pub(crate) struct FakeWsl {
    pub(super) services: Vec<&'static str>,
    pub(super) classes: BTreeMap<&'static str, ClassEntry>,
    pub(super) keys: BTreeMap<AppId, Key>,
    pub(super) default: Option<Vec<u8>>,
    pub(super) packaged: bool,
    pub(super) group: Option<Sid>,
    /// The machine record's two values; `None` when the record's key is gone.
    pub(super) record: Option<WslRecord>,
    /// Calls that fail: `services`, `classes`, `read <id>`, `write <id>`, `record`, ...
    pub(super) fail: Vec<String>,
    /// What a write stores instead of what it was given, when set.
    pub(super) stores: Option<Vec<u8>>,
    /// Each write, as a line.
    pub(super) writes: Vec<String>,
    /// Each read of a value, as a line.
    pub(super) reads: RefCell<Vec<String>>,
}

impl FakeWsl {
    /// The VM's machine (R5c): `WSLService`, both lifted classes on their `AppID`s with the stock
    /// value, the VM's `DefaultLaunchPermission`, no inbox class, and the record with no
    /// targets yet.
    pub(crate) fn lifted() -> FakeWsl {
        let mut fake = FakeWsl {
            services: vec![WSL_SERVICE],
            default: Some(LaunchSd::built_in_default().to_bytes()),
            group: Some(group()),
            record: Some(WslRecord::default()),
            ..FakeWsl::default()
        };
        for (class, id) in [
            (LXSS_USER_SESSION, LXSS_APP),
            (WSLC_SESSION_MANAGER, WSLC_APP),
        ] {
            fake.classes.insert(class, ClassEntry::AppId(app(id)));
            fake.keys.insert(
                app(id),
                Key {
                    service: Some(WSL_SERVICE),
                    value: Some(stock()),
                },
            );
        }
        fake
    }

    /// `id`'s value, parsed.
    pub(super) fn sd(&self, id: &str) -> LaunchSd {
        LaunchSd::parse(self.keys[&app(id)].value.as_ref().unwrap()).unwrap()
    }

    fn failing(&self, call: &str) -> Result<(), WslError> {
        if self.fail.iter().any(|fail| fail == call) {
            return Err(WslError(format!("{call} failed")));
        }
        Ok(())
    }
}

impl WslRead for FakeWsl {
    fn service_exists(&self, name: &str) -> Result<bool, WslError> {
        self.failing(&format!("service {name}"))?;
        Ok(self.services.contains(&name))
    }

    fn class(&self, clsid: &str) -> Result<ClassEntry, WslError> {
        self.failing("classes")?;
        Ok(self
            .classes
            .get(clsid)
            .cloned()
            .unwrap_or(ClassEntry::Missing))
    }

    fn service_app_ids(&self, services: &[&str]) -> Result<Vec<AppId>, WslError> {
        self.failing("services")?;
        let wanted = |key: &Key| {
            key.service
                .is_some_and(|s| services.iter().any(|w| w.eq_ignore_ascii_case(s)))
        };
        Ok(self
            .keys
            .iter()
            .filter(|(_, key)| wanted(key))
            .map(|(id, _)| id.clone())
            .collect())
    }

    fn launch_permission(&self, app_id: &AppId) -> Result<Permission, WslError> {
        self.reads.borrow_mut().push(format!("read {app_id}"));
        self.failing(&format!("read {app_id}"))?;
        Ok(match self.keys.get(app_id) {
            None => Permission::NoKey,
            Some(Key { value: None, .. }) => Permission::Absent,
            Some(Key {
                value: Some(bytes), ..
            }) => Permission::Bytes(bytes.clone()),
        })
    }

    fn packaged_class(&self) -> Result<bool, WslError> {
        self.failing("packaged")?;
        Ok(self.packaged)
    }

    fn group(&self) -> Option<Sid> {
        self.group.clone()
    }
}

impl WslWrite for FakeWsl {
    fn default_launch_permission(&self) -> Result<Option<Vec<u8>>, WslError> {
        self.failing("default")?;
        Ok(self.default.clone())
    }

    fn write_launch_permission(&mut self, app_id: &AppId, bytes: &[u8]) -> Result<(), WslError> {
        self.writes.push(format!("write {app_id}"));
        self.failing(&format!("write {app_id}"))?;
        let stored = self.stores.clone().unwrap_or_else(|| bytes.to_vec());
        let key = self
            .keys
            .get_mut(app_id)
            .ok_or_else(|| WslError("no key".to_owned()))?;
        key.value = Some(stored);
        Ok(())
    }

    fn delete_launch_permission(&mut self, app_id: &AppId) -> Result<(), WslError> {
        self.writes.push(format!("delete {app_id}"));
        self.failing(&format!("delete {app_id}"))?;
        if let Some(key) = self.keys.get_mut(app_id) {
            key.value = None;
        }
        Ok(())
    }

    fn recorded(&self) -> Result<WslRecord, WslError> {
        self.failing("recorded")?;
        Ok(self.record.clone().unwrap_or_default())
    }

    fn record(&mut self, record: &WslRecord) -> Result<(), WslError> {
        self.writes.push("record".to_owned());
        self.failing("record")?;
        let slot = self
            .record
            .as_mut()
            .ok_or_else(|| WslError("the record's key is gone".to_owned()))?;
        *slot = record.clone();
        Ok(())
    }

    fn forget(&mut self) -> Result<(), WslError> {
        self.writes.push("forget".to_owned());
        self.failing("forget")?;
        if let Some(record) = self.record.as_mut() {
            *record = WslRecord::default();
        }
        Ok(())
    }
}

/// A machine with no WSL, for the launch check's tests outside this module.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoWsl;

impl WslRead for NoWsl {
    fn service_exists(&self, _name: &str) -> Result<bool, WslError> {
        Ok(false)
    }

    fn class(&self, _clsid: &str) -> Result<ClassEntry, WslError> {
        Ok(ClassEntry::Missing)
    }

    fn service_app_ids(&self, _services: &[&str]) -> Result<Vec<AppId>, WslError> {
        Ok(Vec::new())
    }

    fn launch_permission(&self, _app_id: &AppId) -> Result<Permission, WslError> {
        Ok(Permission::NoKey)
    }

    fn packaged_class(&self) -> Result<bool, WslError> {
        Ok(false)
    }

    fn group(&self) -> Option<Sid> {
        None
    }
}
