//! The record's key and the vendor key above it, deleted once empty, against a fake registry.

use std::collections::BTreeMap;

use super::{Counts, Keys, VENDOR_KEY, delete_emptied};
use crate::windows::setup::MachineError;

/// The record's key, as the set-up names it.
const RECORD: &str = "SOFTWARE\\Clauseo\\Sandbox";

/// The firm's policy key, which nothing here may touch.
const POLICIES: &str = "SOFTWARE\\Policies\\Clauseo";

/// Keys under `HKLM` by name, each with its value count; subkeys are counted from the names.
/// A delete is refused when the key has a subkey, as `RegDeleteKeyExW` refuses it.
#[derive(Default)]
struct FakeKeys {
    keys: BTreeMap<String, u32>,
    deleted: Vec<String>,
}

impl FakeKeys {
    fn with(keys: &[(&str, u32)]) -> FakeKeys {
        FakeKeys {
            keys: keys
                .iter()
                .map(|(key, values)| ((*key).to_owned(), *values))
                .collect(),
            deleted: Vec::new(),
        }
    }

    fn subkeys(&self, key: &str) -> u32 {
        let prefix = format!("{key}\\");
        let below = self
            .keys
            .keys()
            .filter(|name| name.starts_with(&prefix))
            .count();
        u32::try_from(below).unwrap()
    }

    fn has(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }
}

impl Keys for FakeKeys {
    fn counts(&mut self, key: &str) -> Result<Option<Counts>, MachineError> {
        Ok(self.keys.get(key).map(|values| Counts {
            subkeys: self.subkeys(key),
            values: *values,
        }))
    }

    fn delete(&mut self, key: &str) -> Result<(), MachineError> {
        if self.subkeys(key) > 0 {
            return Err(MachineError::Os {
                call: "RegDeleteKeyExW",
                code: 5,
            });
        }
        self.keys.remove(key);
        self.deleted.push(key.to_owned());
        Ok(())
    }
}

#[test]
fn an_empty_record_key_goes_and_then_the_empty_vendor_key() {
    let mut keys = FakeKeys::with(&[(VENDOR_KEY, 0), (RECORD, 0), (POLICIES, 1)]);
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.deleted, [RECORD, VENDOR_KEY]);
    assert!(keys.has(POLICIES), "the policy key stays");
}

#[test]
fn a_vendor_key_with_a_value_or_another_subkey_stays() {
    let mut keys = FakeKeys::with(&[(VENDOR_KEY, 1), (RECORD, 0)]);
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.deleted, [RECORD]);
    assert!(keys.has(VENDOR_KEY), "a value keeps it");
    let installer = "SOFTWARE\\Clauseo\\Clauseo";
    let mut keys = FakeKeys::with(&[(VENDOR_KEY, 0), (RECORD, 0), (installer, 1)]);
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.deleted, [RECORD]);
    assert!(
        keys.has(VENDOR_KEY) && keys.has(installer),
        "a subkey keeps it"
    );
}

#[test]
fn a_record_key_with_values_left_keeps_both() {
    let mut keys = FakeKeys::with(&[(VENDOR_KEY, 0), (RECORD, 4)]);
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.deleted, Vec::<String>::new());
}

#[test]
fn a_missing_record_key_still_clears_an_empty_vendor_key() {
    let mut keys = FakeKeys::with(&[(VENDOR_KEY, 0)]);
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.deleted, [VENDOR_KEY]);
}

/// A key outside `SOFTWARE\Clauseo` (a test's, say) never reaches the vendor key or any other
/// key above it.
#[test]
fn only_the_vendor_key_above_the_record_is_ever_deleted() {
    let elsewhere = "SOFTWARE\\Policies\\Clauseo\\Sandbox";
    let mut keys = FakeKeys::with(&[(POLICIES, 0), (elsewhere, 0), (VENDOR_KEY, 0)]);
    delete_emptied(&mut keys, elsewhere).unwrap();
    assert_eq!(keys.deleted, [elsewhere]);
    assert!(keys.has(POLICIES) && keys.has(VENDOR_KEY));
}

/// A vendor key that can't be deleted (a subkey made after the count) is logged, and the
/// removal goes on: the record is gone.
#[test]
fn a_refused_vendor_key_delete_is_not_a_failure() {
    struct Racing(FakeKeys);
    impl Keys for Racing {
        fn counts(&mut self, key: &str) -> Result<Option<Counts>, MachineError> {
            self.0.counts(key)
        }
        fn delete(&mut self, key: &str) -> Result<(), MachineError> {
            if key == VENDOR_KEY {
                self.0.keys.insert("SOFTWARE\\Clauseo\\New".to_owned(), 0);
            }
            self.0.delete(key)
        }
    }
    let mut keys = Racing(FakeKeys::with(&[(VENDOR_KEY, 0), (RECORD, 0)]));
    delete_emptied(&mut keys, RECORD).unwrap();
    assert_eq!(keys.0.deleted, [RECORD]);
    assert!(keys.0.has(VENDOR_KEY));
}
