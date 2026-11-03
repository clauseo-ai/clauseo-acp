//! These read the real managed preferences. The only one they write is a throwaway user domain
//! of their own, whose value [`UserDomain`] removes again; Clauseo's domain is never written.

use super::{ManagedPreferences, plist_value};
use crate::policy::managed_prefs::PlistValue;
use crate::policy::{PolicyRead, PolicySource, system_policy_sources};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFPreferencesAppSynchronize,
    CFPreferencesSetAppValue, CFRetained, CFString, CFType,
};

/// A user preferences domain only this test process writes. Dropping it removes what it wrote.
struct UserDomain(CFRetained<CFString>);

impl UserDomain {
    fn new() -> UserDomain {
        let name = format!("ai.clauseo.desktop.shim-test-{}", std::process::id());
        UserDomain(CFString::from_str(&name))
    }

    /// Sets `key` for the current user, as `defaults write` would: no root, and never forced.
    #[expect(
        unsafe_code,
        reason = "CFPreferencesSetAppValue is FFI with a typed value"
    )]
    /// `None` removes it. Whether the write reached the preferences store.
    fn set(&self, key: &str, value: Option<&CFType>) -> bool {
        // SAFETY: the value, when there is one, is a CFString, a property list type.
        unsafe { CFPreferencesSetAppValue(&CFString::from_str(key), value, &self.0) };
        CFPreferencesAppSynchronize(&self.0)
    }
}

impl Drop for UserDomain {
    fn drop(&mut self) {
        let _removed = self.set("ui.theme", None);
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
    assert!(domain.set("ui.theme", Some(CFString::from_str("dark").as_ref())));
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
