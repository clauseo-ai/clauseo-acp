//! The process-wide switch that keeps macOS's legacy keychain dialogs off, so none shows unless
//! the lawyer pressed Allow in the permissions step.
//!
//! Clauseo's keychain items carry an access list naming the build that wrote them; a data read
//! of another build's item (an ad hoc build signs differently every time) makes macOS ask
//! "Clauseo wants to use your confidential information". Several reads run with no action of
//! the lawyer's: the agents' keys and the case-law credential at start, the blocked-site
//! allow-list hash at each host start, the MCP secrets at each launch, the step's own silent
//! check. [`quiet_start`] turns keychain user interaction off for the whole process
//! (`SecKeychainSetUserInteractionAllowed(false)` through security-framework's safe
//! `SecKeychain::disable_user_interaction`), so each of those reads fails with
//! `errSecInteractionNotAllowed` instead of asking, and the step's keychain row explains. A
//! locked login keychain answers the same way, and the Ask covers it too: macOS then asks to
//! unlock.
//!
//! [`with_dialogs_allowed`] is the one way out: the step's Ask runs its reads inside it, with
//! interaction on only for those reads and off again right after the last, even when one
//! panics. Another thread's read in that moment could raise a dialog of its own, so the window
//! is kept to the Ask's reads; the step's silent check waits it out ([`while_quiet`]). One Ask
//! at a time: a second waits for the first.
//!
//! Never `kSecUseAuthenticationUISkip` for these reads instead: with it a would-ask item reads
//! as NOT FOUND (`errSecItemNotFound`, which `keyring` maps to `NoEntry`), and the callers act
//! on "no item": the case-law refresh deletes the credential and saves signed-out, the
//! allow-list writes a new hash over the item, the desktop's key marker clears. With the switch
//! the read fails with `errSecInteractionNotAllowed`, which `keyring` maps to `PlatformFailure`,
//! an error every caller keeps apart from not-found (`codes.rs` keeps the two apart too).
//!
//! The switch sits behind [`Switch`], so its bookkeeping is tested anywhere; outside macOS it
//! does nothing. Its changes are logged on a lifecycle target (host.log keeps them), never a
//! value.

use std::cell::Cell;
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
            .map_err(|error| error.code())
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

/// The switch's state: the lock while dialogs are off, and the Ask's turn.
pub(crate) struct Quiet<S: Switch> {
    /// Held while the dialogs are off.
    lock: Mutex<Option<S::Lock>>,
    /// Held through an Ask's window, and by a silent check waiting one out.
    turn: Mutex<()>,
}

thread_local! {
    /// Whether this thread holds a [`Quiet`]'s turn: a nested call runs as part of it.
    static HOLDS_TURN: Cell<bool> = const { Cell::new(false) };
}

impl<S: Switch> Quiet<S> {
    pub(crate) const fn new() -> Self {
        Self {
            lock: Mutex::new(None),
            turn: Mutex::new(()),
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

    /// Runs `work` with the dialogs on, then turns them off again, also when `work` panics.
    /// One at a time; a call inside another's `work` runs as part of it.
    pub(crate) fn with_dialogs_allowed<T>(&self, work: impl FnOnce() -> T) -> T {
        if HOLDS_TURN.get() {
            return work();
        }
        let _turn = lock(&self.turn);
        // Declared after the turn, so dropped before it: off again while still this call's.
        let _again = Again { quiet: self };
        HOLDS_TURN.set(true);
        drop(lock(&self.lock).take());
        if S::REAL {
            tracing::info!(target: TARGET, allowed = ?S::allowed(), "keychain dialogs on for the Ask");
        }
        work()
    }

    /// Runs `work` once no Ask's window is open, so its reads never meet the dialogs on.
    pub(crate) fn while_quiet<T>(&self, work: impl FnOnce() -> T) -> T {
        if HOLDS_TURN.get() {
            return work();
        }
        let _turn = lock(&self.turn);
        work()
    }

    /// Whether the dialogs are off now, by this bookkeeping: the tests'.
    #[cfg(test)]
    pub(crate) fn is_quiet(&self) -> bool {
        lock(&self.lock).is_some()
    }
}

/// Turns the dialogs off again when dropped: the end of an Ask's window, however it ends.
struct Again<'a, S: Switch> {
    quiet: &'a Quiet<S>,
}

impl<S: Switch> Drop for Again<'_, S> {
    fn drop(&mut self) {
        HOLDS_TURN.set(false);
        self.quiet.start();
    }
}

/// The process's switch.
static QUIET: Quiet<Os> = Quiet::new();

/// Turns keychain dialogs off for the whole process, before any keychain read: no dialog may
/// show unless the lawyer pressed Allow in the permissions step. Idempotent.
pub(crate) fn quiet_start() {
    QUIET.start();
}

/// Runs `work` (the step's Ask) with keychain dialogs on, off again right after.
pub(crate) fn with_dialogs_allowed<T>(work: impl FnOnce() -> T) -> T {
    QUIET.with_dialogs_allowed(work)
}

/// Runs `work` (the step's silent check) once no Ask's window is open.
pub(crate) fn while_quiet<T>(work: impl FnOnce() -> T) -> T {
    QUIET.while_quiet(work)
}
