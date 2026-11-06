//! No failed Windows logon from the browser's start (decision record 0029, "No failed logon at
//! start"). G17's Windows run saw every launch of Chrome and of Edge write event 4625 (logon
//! type 2, status `0xc000006a`, process `chrome.exe` or `msedge.exe`, the signed-in user), so a
//! lockout policy locked the account after five launches.
//!
//! The logon is Chromium's blank-password check: `CheckBlankPasswordWithPrefs`
//! (`chrome/browser/password_manager/password_manager_util_win.cc`) calls
//! `LogonUser(<user>, ".", "", LOGON32_LOGON_INTERACTIVE, ...)` to learn whether the account has
//! no password. The first tab's `ChromeAutofillClient` asks for it at every start: its
//! constructor calls `SupportsDeviceReauth` (`chrome_autofill_client.cc`, when
//! `AutofillAiWalletPrivatePasses` or `AutofillAmbientAutofill` is on), which reaches
//! `CanAuthenticateWithBiometricOrScreenLock`, `AuthenticatorWin::CanAuthenticateWithScreenLock`
//! and `DeviceAuthenticationPresent`. Every way to the logon goes through that one function, and
//! it skips the logon when Local State's `password_manager.os_password_last_changed` is set and
//! not older than the account's password (`NetUserGetInfo`'s age, which makes no logon), reading
//! the answer from `password_manager.os_password_blank` instead. The guard writes both into the
//! new profile's `Local State`: the time as large as it goes, the answer "not blank". Before a
//! restart on the kept profile (lazy start) it puts back whichever is missing or unreadable
//! ([`ready_local_state`]). A domain-joined machine skips the check already
//! (`base::win::IsEnrolledToDomain`).
//!
//! The profile also turns off Chrome's password manager: the agent saves no passwords.
//!
//! Named for a later merge into `quiet.rs`'s profile preferences.

/// A new profile's `Local State`. Chrome stores an int64 preference as a decimal string
/// (`base::ValueToInt64` reads only a string; another type falls back to the default, 0, which
/// would recheck).
pub(crate) fn local_state() -> serde_json::Value {
    serde_json::json!({
        "password_manager": {
            "os_password_blank": false,
            "os_password_last_changed": i64::MAX.to_string(),
        },
    })
}

/// The `Local State` for a restart on the kept profile: Chrome's own (`kept`), everything in it
/// kept, with [`local_state`]'s two keys put back where missing or unreadable. Chrome's rewrite
/// of them is kept: after its check it stores the password's real time plus 1 s and the answer
/// it read (`password_manager_util_win.cc` `CheckBlankPasswordWithPrefs`), which still skips the
/// logon while the account's password is unchanged. No readable `Local State`: [`local_state`].
pub(crate) fn ready_local_state(kept: Option<serde_json::Value>) -> serde_json::Value {
    let Some(mut state) = kept.filter(serde_json::Value::is_object) else {
        return local_state();
    };
    let ours = local_state();
    let manager = &mut state["password_manager"];
    if !manager.is_object() {
        *manager = serde_json::json!({});
    }
    let changed = &manager["os_password_last_changed"];
    let readable = changed
        .as_str()
        .and_then(|text| text.parse::<i64>().ok())
        .is_some_and(|time| time > 0);
    if !readable {
        manager["os_password_last_changed"] =
            ours["password_manager"]["os_password_last_changed"].clone();
    }
    if !manager["os_password_blank"].is_boolean() {
        manager["os_password_blank"] = ours["password_manager"]["os_password_blank"].clone();
    }
    state
}

/// `Default/Preferences` entries, merged into the profile's: Chrome's password manager off
/// (`credentials_enable_service`), no automatic sign-in with a saved password, and no passkeys
/// from Chrome's own store (`credentials_enable_passkeys`).
pub(crate) fn preferences() -> serde_json::Value {
    serde_json::json!({
        "credentials_enable_service": false,
        "credentials_enable_autosignin": false,
        "credentials_enable_passkeys": false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::chrome::ready_profile;
    use super::{local_state, ready_local_state};
    use crate::write;

    /// The written profile holds the setting that skips the logon, in the type Chrome reads, and
    /// the password manager off, beside the download preferences and `quiet.rs`'s.
    #[test]
    fn a_new_profile_skips_the_blank_password_logon() {
        let dir = std::env::temp_dir().join(format!("clauseo-mcp-logon-{}", std::process::id()));
        let profile = dir.join("chrome-profile");
        ready_profile(&profile, &profile.join("staging"), &dir.join("downloads")).unwrap();
        let local = read(&profile.join("Local State"));
        assert_eq!(local, local_state());
        let manager = &local["password_manager"];
        assert_eq!(manager["os_password_blank"], serde_json::json!(false));
        assert_eq!(
            manager["os_password_last_changed"],
            serde_json::json!("9223372036854775807")
        );
        let preferences = read(&profile.join("Default").join("Preferences"));
        assert_logon_preferences(&preferences);
        assert!(preferences["download"]["default_directory"].is_string());
        // "No background hosts" (`quiet.rs`) arrives beside it, its keys untouched.
        assert_eq!(
            preferences["safebrowsing"]["enabled"],
            serde_json::json!(false)
        );
        write::remove_dir_all(&dir).unwrap();
    }

    /// Lazy start's restart on the kept profile: Chrome's own `Local State` and `Preferences`
    /// stay, its rewrite of the logon keys too, and whatever it dropped or turned back on is put
    /// back.
    #[test]
    fn a_restart_on_the_kept_profile_keeps_the_logon_keys() {
        let dir =
            std::env::temp_dir().join(format!("clauseo-mcp-logon-again-{}", std::process::id()));
        let (profile, staging) = (dir.join("chrome-profile"), dir.join("chrome-profile/s"));
        ready_profile(&profile, &staging, &dir.join("downloads")).unwrap();
        // Chrome's rewrite: its own keys, the logon keys gone, the password manager back on.
        let state = profile.join("Local State");
        let rewritten = serde_json::json!({"os_crypt": {"encrypted_key": "k"}});
        write::write(&state, rewritten.to_string()).unwrap();
        let file = profile.join("Default").join("Preferences");
        let preferences = serde_json::json!({"credentials_enable_service": true, "own": 1});
        write::write(&file, preferences.to_string()).unwrap();
        ready_profile(&profile, &staging, &dir.join("downloads")).unwrap();
        let local = read(&state);
        assert_eq!(local["os_crypt"]["encrypted_key"], "k");
        assert_eq!(local["password_manager"], local_state()["password_manager"]);
        let preferences = read(&file);
        assert_eq!(preferences["own"], 1);
        assert_logon_preferences(&preferences);
        // Chrome's own check, written back: kept.
        let checked = serde_json::json!({"password_manager": {
            "os_password_blank": false,
            "os_password_last_changed": "13380000000000000",
        }});
        write::write(&state, checked.to_string()).unwrap();
        ready_profile(&profile, &staging, &dir.join("downloads")).unwrap();
        assert_eq!(read(&state), checked);
        write::remove_dir_all(&dir).unwrap();
    }

    /// A value Chrome would not read as a time (a number, 0, text) or no `Local State` at all is
    /// replaced with the guard's; Chrome's own answer is kept.
    #[test]
    fn unreadable_logon_keys_are_put_back() {
        let ours = local_state();
        assert_eq!(ready_local_state(None), ours);
        assert_eq!(ready_local_state(Some(serde_json::json!([1]))), ours);
        for changed in [
            serde_json::json!(13_380_000_000_000_000_i64),
            serde_json::json!("0"),
            serde_json::json!("soon"),
            serde_json::Value::Null,
        ] {
            let kept = serde_json::json!({"password_manager": {
                "os_password_blank": true,
                "os_password_last_changed": changed,
            }});
            let ready = ready_local_state(Some(kept));
            let manager = &ready["password_manager"];
            assert_eq!(
                manager["os_password_last_changed"],
                ours["password_manager"]["os_password_last_changed"]
            );
            assert_eq!(manager["os_password_blank"], true);
        }
        let ready = ready_local_state(Some(serde_json::json!({"password_manager": 3})));
        assert_eq!(ready["password_manager"], ours["password_manager"]);
    }

    fn read(path: &std::path::Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn assert_logon_preferences(preferences: &serde_json::Value) {
        for off in [
            "credentials_enable_service",
            "credentials_enable_autosignin",
            "credentials_enable_passkeys",
        ] {
            assert_eq!(preferences[off], serde_json::json!(false), "{off}");
        }
    }
}
