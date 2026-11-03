//! These read the real managed preferences. The only one they write is a throwaway user domain
//! of their own, which [`UserDomain`] removes again, plist file included; Clauseo's domain is
//! never written. A run killed before the drop (SIGKILL, such as a nextest timeout) can leave
//! `~/Library/Preferences/ai.clauseo.desktop.shim-test-<pid>.plist` behind.

use super::{ManagedPreferences, plist_value};
use crate::policy::managed_prefs::PlistValue;
use crate::policy::{PolicyRead, PolicySource, system_policy_sources};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFPreferencesAppSynchronize,
    CFPreferencesSetAppValue, CFRetained, CFString, CFType,
};
use std::path::PathBuf;

/// A user preferences domain only this test process writes, named by its pid so parallel runs
/// (`cargo mutants -j N`) do not share it. Dropping it removes what it wrote.
struct UserDomain(CFRetained<CFString>);

impl UserDomain {
    fn new() -> UserDomain {
        let name = format!("ai.clauseo.desktop.shim-test-{}", std::process::id());
        UserDomain(CFString::from_str(&name))
    }

    /// Sets `key` for the current user, as `defaults write` would: no root, and never forced.
    /// `None` removes it. Whether the write reached the preferences store.
    #[expect(
        unsafe_code,
        reason = "safe: the value is always a CFString, a property-list type"
    )]
    fn set(&self, key: &str, value: Option<&CFString>) -> bool {
        // SAFETY: the value, when there is one, is a CFString, a property-list type.
        unsafe {
            CFPreferencesSetAppValue(&CFString::from_str(key), value.map(AsRef::as_ref), &self.0);
        };
        CFPreferencesAppSynchronize(&self.0)
    }

    /// The domain's plist file, which the preferences store may leave once the domain is empty.
    fn plist(&self) -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(format!("Library/Preferences/{}.plist", self.0)))
    }
}

impl Drop for UserDomain {
    #[expect(
        clippy::disallowed_methods,
        reason = "a test removes the scratch plist file it made"
    )]
    fn drop(&mut self) {
        let _removed = self.set("ui.theme", None);
        if let Some(plist) = self.plist() {
            let _deleted = std::fs::remove_file(plist);
        }
    }
}

#[test]
fn system_policy_absent_on_clean_machine() {
    let sources = system_policy_sources();
    let names: Vec<_> = sources
        .iter()
        .map(|source| source.name().to_owned())
        .collect();
    assert_eq!(
        names,
        [
            "managed preferences",
            "/Library/Application Support/Clauseo/policy.json"
        ]
    );
    for source in &sources {
        assert!(
            matches!(source.read(), PolicyRead::Absent),
            "{}",
            source.name()
        );
    }
}

#[test]
fn managed_prefs_shim_reads_nothing_from_an_empty_domain() {
    let source = ManagedPreferences {
        domain: "ai.clauseo.desktop.no-such-domain".to_owned(),
    };
    assert_eq!(source.values(), []);
    assert!(matches!(source.read(), PolicyRead::Absent));
}

#[test]
fn managed_prefs_shim_reads_a_user_value_but_not_as_policy() {
    let domain = UserDomain::new();
    assert!(domain.set("ui.theme", Some(&CFString::from_str("dark"))));
    let source = ManagedPreferences {
        domain: domain.0.to_string(),
    };
    assert_eq!(
        source.values(),
        [(
            "ui.theme".to_owned(),
            PlistValue::String("dark".to_owned()),
            false
        )]
    );
    // Fails closed: a value nothing forced is the user's, not policy.
    assert!(matches!(source.read(), PolicyRead::Absent));
}

#[test]
fn managed_prefs_shim_converts_each_type() {
    let yes: &CFType = CFBoolean::new(true).as_ref();
    assert_eq!(plist_value(yes), PlistValue::Bool(true));
    let int = CFNumber::new_i64(-4);
    assert_eq!(plist_value(int.as_ref()), PlistValue::Integer(-4));
    let real = CFNumber::new_f64(2.5);
    assert_eq!(plist_value(real.as_ref()), PlistValue::Real(2.5));
    let text = CFString::from_str("dark");
    assert_eq!(
        plist_value(text.as_ref()),
        PlistValue::String("dark".to_owned())
    );
    let array = CFArray::<CFType>::from_objects(&[text.as_ref(), int.as_ref()]);
    assert_eq!(
        plist_value(array.as_ref()),
        PlistValue::Array(vec![
            PlistValue::String("dark".to_owned()),
            PlistValue::Integer(-4)
        ])
    );
    let key = CFString::from_str("theme");
    let dict = CFDictionary::<CFType, CFType>::from_slices(&[key.as_ref()], &[text.as_ref()]);
    assert_eq!(
        plist_value(dict.as_ref()),
        PlistValue::Dict(vec![(
            "theme".to_owned(),
            PlistValue::String("dark".to_owned())
        )])
    );
}
