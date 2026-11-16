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
//! keychain opened by path under the home folder of the user record (`getpwuid_r`, never
//! `$HOME`), since the default can be another, unlocked keychain. The call reads the state and
//! never unlocks or prompts (Apple: "Retrieves status information about a keychain";
//! `kSecUnlockStateStatus` is bit 0). A failed call, no user record or no login keychain file
//! counts as locked: could not tell, so the background run is skipped. Other keychains in the
//! search list are not read: `/Library/Keychains/System.keychain` reads locked (0x2) to every
//! user (K0, 2026-11-15), and a locked extra keychain raised no dialog (VM row M-c).
//! (`security show-keychain-info` cannot stand in: in the lawyer's session it exits 0 locked or
//! unlocked, VM 2026-11-15.)
//!
//! The one case left open (review of #1132): a third keychain, neither the default nor the login
//! one, that is locked and holds an item with Claude Code's service and account. `security`
//! searches the whole list, so a read of that item could still show an unlock dialog with the
//! guard reading unlocked. Accepted: VM row M-c showed a locked extra keychain without such an
//! item raises no dialog, and nothing seen so far puts Claude Code's item in one.

#[cfg(any(target_os = "macos", all(test, unix)))]
use std::path::Path;

use crate::config::AgentsConfig;
use crate::platform::Os;

/// `kSecUnlockStateStatus`: the keychain is unlocked.
#[cfg(any(target_os = "macos", all(test, unix)))]
const UNLOCKED: u32 = 1;

/// The login keychain's file names in `<home>/Library/Keychains`, the current one first.
#[cfg(any(target_os = "macos", all(test, unix)))]
const LOGIN_NAMES: [&str; 2] = ["login.keychain-db", "login.keychain"];

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

/// Whether a `SecKeychainStatus` (`None`: the call failed) reads unlocked.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn unlocked(status: Option<u32>) -> bool {
    status.is_some_and(|status| status & UNLOCKED != 0)
}

/// The decision, with the macOS reads passed in: `default` is the default keychain's status,
/// `home` the user record's home folder (`None`: the lookup failed), `is_file` whether a file
/// exists, and `status_at` the status of the keychain opened at a path. Locked unless the
/// default reads unlocked and so does the first login keychain file found.
#[cfg(any(target_os = "macos", all(test, unix)))]
fn decide(
    default: Option<u32>,
    home: Option<&Path>,
    is_file: impl Fn(&Path) -> bool,
    status_at: impl FnOnce(&Path) -> Option<u32>,
) -> bool {
    if !unlocked(default) {
        return true;
    }
    let Some(home) = home else {
        return true;
    };
    let folder = home.join("Library").join("Keychains");
    let Some(login) = LOGIN_NAMES
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| is_file(path))
    else {
        return true;
    };
    !unlocked(status_at(&login))
}

/// [`user_home_uncached`], looked up once per process. The limits readers call
/// [`login_keychain_locked`] inside a future on the host actor's thread, and `getpwuid_r` can
/// reach the directory service (a network-bound Mac), so it is never asked again. A failed
/// lookup is cached too: `None`, which [`decide`] reads as locked for the rest of the run.
#[cfg(target_os = "macos")]
fn user_home() -> Option<&'static Path> {
    static HOME: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(user_home_uncached).as_deref()
}

/// The home folder of the user record of the real user id (`getpwuid_r`), never `$HOME` or
/// `CFFIXED_USER_HOME`, which the environment sets: `None` when the lookup fails, finds no user
/// or an empty folder.
#[cfg(target_os = "macos")]
fn user_home_uncached() -> Option<std::path::PathBuf> {
    use nix::unistd::{User, getuid};
    User::from_uid(getuid())
        .ok()
        .flatten()
        .map(|user| user.dir)
        .filter(|dir| !dir.as_os_str().is_empty())
}

/// Whether the default keychain or the user's login keychain is locked, by
/// `SecKeychainGetStatus`, which never prompts: on the default keychain (`NULL`), then on
/// `<home>/Library/Keychains/login.keychain-db` (else the older `login.keychain`) opened by path
/// with `SecKeychainOpen`, `<home>` from the user record ([`decide`]). True (could not tell)
/// when a call fails, there is no user record or neither login keychain file exists. Always
/// false outside macOS. Public for desktop's limits readers, which skip Claude's background read
/// while it is locked.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "SecKeychainGetStatus is the Security framework's only lock-state call that never prompts"
)]
pub fn login_keychain_locked() -> bool {
    use std::ffi::{CString, c_char, c_void};
    use std::os::unix::ffi::OsStrExt;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeychainOpen(path_name: *const c_char, keychain: *mut *const c_void) -> i32;
        fn SecKeychainGetStatus(keychain: *const c_void, status: *mut u32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    let read = |keychain: *const c_void| {
        let mut status: u32 = 0;
        // SAFETY: `keychain` is null (the default keychain) or one `SecKeychainOpen` returned and
        // not yet released; `status` is a valid u32 the call writes.
        let code = unsafe { SecKeychainGetStatus(keychain, &raw mut status) };
        (code == 0).then_some(status)
    };
    let status_at = |path: &Path| {
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut keychain: *const c_void = std::ptr::null();
        // SAFETY: `path` is a NUL-terminated string that outlives the call; `keychain` is a
        // valid `SecKeychainRef` slot the call writes.
        let code = unsafe { SecKeychainOpen(path.as_ptr(), &raw mut keychain) };
        if keychain.is_null() {
            return None;
        }
        let status = if code == 0 { read(keychain) } else { None };
        // SAFETY: `keychain` is the one `SecKeychainOpen` returned, which the caller must
        // release; it is released once here and not used after.
        unsafe { CFRelease(keychain) };
        status
    };
    decide(
        read(std::ptr::null()),
        user_home(),
        Path::is_file,
        status_at,
    )
}

/// No keychain to lock outside macOS (a config saying macOS on another OS is a test's).
#[cfg(not(target_os = "macos"))]
pub fn login_keychain_locked() -> bool {
    false
}

// Unix only: the tests hold the user's home as a Unix path (`/Users/…`), which Windows does not
// take as absolute (xtask's unix-paths check); the code they test is macOS's.
#[cfg(all(test, unix))]
mod tests;
