//! What a silent keychain read's answer means for the step (`mac.rs`), as a pure mapping, so
//! it is tested anywhere.
//!
//! Probed on a Mac (macOS 27.2) with keychain dialogs off (`quiet.rs`): an item whose access
//! list does not hold this build answers `errSecAuthFailed` (-25293), through the legacy read
//! and `SecItemCopyMatching` alike; so does a locked login keychain. An item any app may read
//! answers 0, a missing one `errSecItemNotFound`; `SecItemDelete` of another build's item
//! answers `errSecNoStorageAccess` (-25244), which counts as refused, never as gone.
//! `errSecInteractionNotAllowed` and `errSecInDarkWake` are the documented answers when a
//! dialog was needed but could not show, so they count as would-ask too.

/// `errSecItemNotFound`.
pub(super) const ITEM_NOT_FOUND: i32 = -25300;
/// `errSecNoStorageAccess`: with dialogs off, `SecItemDelete` of another build's item on an
/// unlocked keychain (probed on a Mac); refused, never gone.
pub(super) const NO_STORAGE_ACCESS: i32 = -25244;
/// `errSecAuthFailed`: with dialogs off, another build's access list, or a locked keychain.
pub(super) const AUTH_FAILED: i32 = -25293;
/// `errSecInteractionNotAllowed`: answering needed a dialog, with dialogs off.
pub(super) const INTERACTION_NOT_ALLOWED: i32 = -25308;
/// `errSecInDarkWake`: the Mac is in dark wake, where no dialog can show.
pub(super) const IN_DARK_WAKE: i32 = -25315;

/// Whether an item reads without a dialog, from its data read with dialogs off: `Ok(data)`
/// whether a value came back, `Err(code)` the OS's error. `Some(true)` it reads (or has gone
/// since it was listed: nothing left to ask for, as the Ask counts it), `Some(false)` it would
/// ask (another build's access list or a locked keychain, both `errSecAuthFailed`; a storage
/// access refusal; or a dialog that could not show: the Ask covers each, macOS then asks to
/// allow or to unlock), and `None` the read said neither (an answer with no value, another
/// error).
pub(super) fn silent_answer(read: Result<bool, i32>) -> Option<bool> {
    match read {
        Ok(true) | Err(ITEM_NOT_FOUND) => Some(true),
        Err(AUTH_FAILED | NO_STORAGE_ACCESS | INTERACTION_NOT_ALLOWED | IN_DARK_WAKE) => {
            Some(false)
        }
        Ok(false) | Err(_) => None,
    }
}
