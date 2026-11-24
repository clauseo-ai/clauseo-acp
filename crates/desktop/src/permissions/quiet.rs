//! The process-wide switch that keeps macOS's legacy keychain dialogs off in the app's own
//! process for its whole life, so none shows unless the lawyer pressed Allow in the permissions
//! step.
//!
//! Clauseo's keychain items carry an access list naming the build that wrote them; a data read
//! of another build's item (an ad hoc build signs differently every time) makes macOS ask
//! "Clauseo wants to use your confidential information". Several reads run with no action of
//! the lawyer's: the agents' keys and the case-law credential at start, the blocked-site
//! allow-list hash at each host start, the MCP secrets at each launch, the step's own silent
//! check. [`quiet_start`] turns keychain user interaction off for the whole process
//! (`SecKeychainSetUserInteractionAllowed(false)` through security-framework's safe
//! `SecKeychain::disable_user_interaction`), so each of those reads fails instead of asking,
//! and the step's keychain row explains. Probed on a Mac: the read fails with
//! `errSecAuthFailed` (-25293), for another build's item and for a locked keychain alike
//! (`codes.rs`).
//!
//! Nothing in this process turns dialogs back on. The step's Ask reads in a child process of
//! its own (`ask_child.rs`), which starts with dialogs on: macOS's dialog shows there, and
//! only while the lawyer's Ask runs; another thread of the app never meets dialogs on.
//!
//! Never `kSecUseAuthenticationUISkip` for these reads instead: with it a would-ask item reads
//! as NOT FOUND (`errSecItemNotFound`, which `keyring` maps to `NoEntry`), and the callers act
//! on "no item": the case-law refresh deletes the credential and saves signed-out, the
//! allow-list writes a new hash over the item, the desktop's key marker clears. With the switch
//! the read fails with `errSecAuthFailed`, which `keyring`'s macOS store maps to
//! `PlatformFailure` (only -25300 is `NoEntry`), an error every caller keeps apart from
//! not-found (`codes.rs` keeps the two apart too).
//!
//! The switch sits behind [`Switch`], so its bookkeeping is tested anywhere; outside macOS it
//! does nothing. Its changes are logged on a lifecycle target (host.log keeps them), never a
//! value.

use std::sync::Mutex;

use crate::bridge::lock;

/// The log target of the switch's changes: host.log keeps INFO from `::lifecycle` targets.
const TARGET: &str = "clauseo_desktop::permissions::lifecycle";

/// The OS's switch for keychain dialogs.
pub(crate) trait Switch {
    /// What keeps the dialogs off while held; dropping it turns them on.
    type Lock: Send;
    /// Whether the switch is a real one, whose changes are worth a log line.
    const REAL: bool;
    /// Turns the dialogs off: the lock, or the OS's error code.
    fn disable() -> Result<Self::Lock, i32>;
    /// Whether the dialogs are allowed now, when that can be read.
    fn allowed() -> Option<bool>;
}

/// macOS: the legacy keychain's user-interaction switch.
#[cfg(target_os = "macos")]
pub(crate) struct Os;

#[cfg(target_os = "macos")]
impl Switch for Os {
    type Lock = security_framework::os::macos::keychain::KeychainUserInteractionLock;
    const REAL: bool = true;
    fn disable() -> Result<Self::Lock, i32> {
        security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()
            .map_err(security_framework::base::Error::code)
    }
    fn allowed() -> Option<bool> {
        security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed().ok()
    }
}

/// Elsewhere: no keychain dialog to keep off.
#[cfg(not(target_os = "macos"))]
pub(crate) struct Os;

#[cfg(not(target_os = "macos"))]
impl Switch for Os {
    type Lock = ();
    const REAL: bool = false;
    fn disable() -> Result<Self::Lock, i32> {
        Ok(())
    }
    fn allowed() -> Option<bool> {
        None
    }
}

/// The switch's state: the lock while dialogs are off (kept for the process's life).
pub(crate) struct Quiet<S: Switch> {
    lock: Mutex<Option<S::Lock>>,
}

impl<S: Switch> Quiet<S> {
    pub(crate) const fn new() -> Self {
        Self {
            lock: Mutex::new(None),
        }
    }

    /// Turns the dialogs off, unless they are already.
    pub(crate) fn start(&self) {
        let mut held = lock(&self.lock);
        if held.is_some() {
            return;
        }
        match S::disable() {
            Ok(off) => {
                *held = Some(off);
                if S::REAL {
                    tracing::info!(
                        target: TARGET,
                        allowed = ?S::allowed(),
                        "keychain dialogs off"
                    );
                }
            }
            Err(code) => tracing::warn!(target: TARGET, code, "keychain dialogs not turned off"),
        }
    }

    /// Whether the dialogs are off now, by this bookkeeping: the tests'.
    #[cfg(test)]
    pub(crate) fn is_quiet(&self) -> bool {
        lock(&self.lock).is_some()
    }
}

/// The process's switch.
static QUIET: Quiet<Os> = Quiet::new();

/// Turns keychain dialogs off for the whole process, before any keychain read: no dialog may
/// show unless the lawyer pressed Allow in the permissions step. Idempotent.
pub(crate) fn quiet_start() {
    QUIET.start();
}
