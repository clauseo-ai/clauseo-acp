//! macOS' policy sources: managed preferences, then
//! `/Library/Application Support/Clauseo/policy.json`. This module only collects the raw
//! values; [`super::managed_prefs::parse`] reads them.

#[cfg(test)]
mod tests;

use super::managed_prefs::{self, AGENTS_KEY, PlistValue, SOURCE};
use super::{FilePolicy, PolicyRead, PolicySource};
use crate::SystemFiles;
use crate::leaf::KEYS;
use crate::locations::IDENTIFIER;
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFPreferencesAppValueIsForced,
    CFPreferencesCopyAppValue, CFRetained, CFString, CFType,
};
use std::path::PathBuf;

/// Managed preferences, then the `policy.json` in `/Library/Application Support/Clauseo`.
pub(super) fn sources() -> Vec<Box<dyn PolicySource>> {
    vec![
        Box::new(ManagedPreferences {
            domain: IDENTIFIER.to_owned(),
        }),
        Box::new(FilePolicy::new(
            PathBuf::from("/Library/Application Support/Clauseo/policy.json"),
            SystemFiles,
        )),
    ]
}

/// The managed preferences of one domain.
pub(super) struct ManagedPreferences {
    pub(super) domain: String,
}

impl PolicySource for ManagedPreferences {
    fn name(&self) -> &str {
        SOURCE
    }

    fn read(&self) -> PolicyRead {
        managed_prefs::parse(self.values())
    }
}

impl ManagedPreferences {
    /// Each key a setting can have, with its value and whether it is forced. Managed
    /// preferences are read key by key, so only these keys are seen.
    fn values(&self) -> Vec<(String, PlistValue, bool)> {
        let domain = CFString::from_str(&self.domain);
        KEYS.iter()
            .map(|&(key, _)| key)
            .chain([AGENTS_KEY])
            .filter_map(|key| {
                let name = CFString::from_str(key);
                let value = CFPreferencesCopyAppValue(&name, &domain)?;
                let forced = CFPreferencesAppValueIsForced(&name, &domain);
                Some((key.to_owned(), plist_value(&value), forced))
            })
            .collect()
    }
}

/// A Core Foundation property list value as a [`PlistValue`].
fn plist_value(value: &CFType) -> PlistValue {
    if let Some(flag) = value.downcast_ref::<CFBoolean>() {
        PlistValue::Bool(flag.as_bool())
    } else if let Some(number) = value.downcast_ref::<CFNumber>() {
        if number.is_float_type() {
            number
                .as_f64()
                .map_or(PlistValue::Other("number"), PlistValue::Real)
        } else {
            number
                .as_i64()
                .map_or(PlistValue::Other("number"), PlistValue::Integer)
        }
    } else if let Some(text) = value.downcast_ref::<CFString>() {
        PlistValue::String(text.to_string())
    } else if let Some(array) = value.downcast_ref::<CFArray>() {
        PlistValue::Array(items(array).iter().map(|item| plist_value(item)).collect())
    } else if let Some(dict) = value.downcast_ref::<CFDictionary>() {
        let (keys, values) = entries(dict);
        PlistValue::Dict(
            keys.iter()
                .zip(&values)
                .map(|(key, value)| (key_string(key), plist_value(value)))
                .collect(),
        )
    } else {
        PlistValue::Other("date or data")
    }
}

/// A dictionary key as a string; property list keys are always strings.
fn key_string(key: &CFType) -> String {
    key.downcast_ref::<CFString>()
        .map_or_else(String::new, ToString::to_string)
}

/// The items of a property list array.
#[expect(unsafe_code, reason = "a property list array holds only CF objects")]
fn items(array: &CFArray) -> Vec<CFRetained<CFType>> {
    // SAFETY: every item of a property list array is a property list object, a CF object, so
    // the array may be read as one of `CFType`.
    unsafe { array.cast_unchecked::<CFType>() }.to_vec()
}

/// The keys and values of a property list dictionary.
#[expect(
    unsafe_code,
    reason = "a property list dictionary holds only CF objects"
)]
fn entries(dict: &CFDictionary) -> (Vec<CFRetained<CFType>>, Vec<CFRetained<CFType>>) {
    // SAFETY: every key and value of a property list dictionary is a property list object, a
    // CF object, so the dictionary may be read as one from `CFType` to `CFType`.
    unsafe { dict.cast_unchecked::<CFType, CFType>() }.to_vecs()
}
