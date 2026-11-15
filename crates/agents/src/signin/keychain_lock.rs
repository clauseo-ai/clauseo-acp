//! The locked-keychain guard for background Claude runs on macOS (2026-11-15, VM finding): with
//! the login keychain locked, Claude Code's own `security find-generic-password -w` of its item
//! (Clauseo's own, suffixed, or the lawyer's under reuse) shows "security wants to use the
//! 'login' keychain", at times the lawyer did nothing. So the status check the app starts by
//! itself (`Task::Status`, at start, on a window's Plans and in its background re-checks) runs no
//! Claude while the default keychain is locked: it could not tell, the state stays, and a later
//! check asks again. Turns, sign-ins, "Use it" and "Look for my login" are the lawyer's own acts
//! and are not guarded here.
//!
//! The lock state comes from `SecKeychainGetStatus` on the default keychain, which reads the
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
/// and the default keychain is locked. A test sets the answer with
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

/// Whether the default (login) keychain is locked, by `SecKeychainGetStatus(NULL, …)`, which
/// never prompts; true (could not tell) when the call fails. Always false outside macOS. Public
/// for desktop's limits readers, which skip Claude's background read while it is locked.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "SecKeychainGetStatus is the Security framework's only lock-state call that never prompts"
)]
pub fn login_keychain_locked() -> bool {
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeychainGetStatus(keychain: *const std::ffi::c_void, status: *mut u32) -> i32;
    }
    let mut status: u32 = 0;
    // SAFETY: a null keychain means the default one; `status` is a valid u32 the call writes.
    let code = unsafe { SecKeychainGetStatus(std::ptr::null(), &raw mut status) };
    code != 0 || status & UNLOCKED == 0
}

/// No keychain to lock outside macOS (a config saying macOS on another OS is a test's).
#[cfg(not(target_os = "macos"))]
pub fn login_keychain_locked() -> bool {
    false
}
