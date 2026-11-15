//! The locked-keychain guard for background Claude runs on macOS (2026-11-15, VM finding): with
//! the login keychain locked, Claude Code's own `security find-generic-password -w` of its item
//! (Clauseo's own, suffixed, or the lawyer's under reuse) shows "security wants to use the
//! 'login' keychain", at times the lawyer did nothing. So the status check the app starts by
//! itself (`Task::Status`, at start, on a window's Plans and in its background re-checks) runs no
//! Claude while the default keychain or the login keychain is locked: it could not tell, the
//! state stays, and a later check asks again. Turns, sign-ins, "Use it" and "Look for my login"
//! are the lawyer's own acts and are not guarded here.
//!
//! The lock state comes from `SecKeychainGetStatus` on the default keychain and on the login
//! keychain opened by path (the default can be another, unlocked keychain), which reads the
//! state and never unlocks or prompts (Apple: "Retrieves status information about a keychain";
//! `kSecUnlockStateStatus` is bit 0). A failed call counts as locked: could not tell, so the
//! background run is skipped. (`security show-keychain-info` cannot stand in: in the lawyer's
//! session it exits 0 locked or unlocked, VM 2026-11-15.)

use crate::config::AgentsConfig;
use crate::platform::Os;

/// `kSecUnlockStateStatus`: the keychain is unlocked.
#[cfg(target_os = "macos")]
const UNLOCKED: u32 = 1;

/// Whether the background status check must skip Claude's runs: `config`'s platform is macOS
/// and the default or login keychain is locked. A test sets the answer with
/// `SigninConfig::keychain_locked`; with none, a test never reads the machine's keychain.
pub(crate) fn locked(config: &AgentsConfig) -> bool {
    if config.platform.os != Os::Darwin {
        return false;
    }
    #[cfg(feature = "test-support")]
    if let Some(locked) = config.signin.keychain_locked {
        return locked;
    }
    login_keychain_locked()
}

/// Whether the default keychain or the user's login keychain is locked, by
/// `SecKeychainGetStatus`, which never prompts: first on the default keychain (`NULL`), then on
/// `~/Library/Keychains/login.keychain-db` (else the older `login.keychain`) opened by path with
/// `SecKeychainOpen`, since the default keychain can be another, unlocked one while the login
/// keychain, where Claude's item lives, is locked (VM 2026-11-15). True (could not tell) when a
/// call fails, there is no home folder or neither login keychain file exists. Always false
/// outside macOS. Public for desktop's limits readers, which skip Claude's background read while
/// it is locked.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "SecKeychainGetStatus is the Security framework's only lock-state call that never prompts"
)]
pub fn login_keychain_locked() -> bool {
    use std::ffi::{CString, c_char, c_void};
    use std::os::unix::ffi::OsStringExt;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeychainOpen(path_name: *const c_char, keychain: *mut *const c_void) -> i32;
        fn SecKeychainGetStatus(keychain: *const c_void, status: *mut u32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    let unlocked = |keychain: *const c_void| {
        let mut status: u32 = 0;
        // SAFETY: `keychain` is null (the default keychain) or one `SecKeychainOpen` returned and
        // not yet released; `status` is a valid u32 the call writes.
        let code = unsafe { SecKeychainGetStatus(keychain, &raw mut status) };
        code == 0 && status & UNLOCKED != 0
    };

    if !unlocked(std::ptr::null()) {
        return true;
    }
    let Some(home) = std::env::home_dir() else {
        return true;
    };
    let folder = home.join("Library").join("Keychains");
    let Some(path) = ["login.keychain-db", "login.keychain"]
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
    else {
        return true;
    };
    let Ok(path) = CString::new(path.into_os_string().into_vec()) else {
        return true;
    };
    let mut login: *const c_void = std::ptr::null();
    // SAFETY: `path` is a NUL-terminated string that outlives the call; `login` is a valid
    // `SecKeychainRef` slot the call writes.
    let code = unsafe { SecKeychainOpen(path.as_ptr(), &raw mut login) };
    if code != 0 || login.is_null() {
        return true;
    }
    let login_unlocked = unlocked(login);
    // SAFETY: `login` is the keychain `SecKeychainOpen` returned, which the caller must release;
    // it is released once here and not used after.
    unsafe { CFRelease(login) };
    !login_unlocked
}

/// No keychain to lock outside macOS (a config saying macOS on another OS is a test's).
#[cfg(not(target_os = "macos"))]
pub fn login_keychain_locked() -> bool {
    false
}
