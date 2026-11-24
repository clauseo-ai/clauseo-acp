//! What a silent keychain read's answer means for the step (`mac.rs`), as a pure mapping, so
//! it is tested anywhere.

/// `errSecItemNotFound`.
pub(super) const ITEM_NOT_FOUND: i32 = -25300;
/// `errSecInteractionNotAllowed`: answering needed a dialog, with dialogs off (`quiet.rs`).
pub(super) const INTERACTION_NOT_ALLOWED: i32 = -25308;

/// Whether an item reads without a dialog, from its data read with dialogs off: `Ok(data)`
/// whether a value came back, `Err(code)` the OS's error. `Some(true)` it reads (or has gone
/// since it was listed: nothing left to ask for, as the Ask's `read` counts it), `Some(false)`
/// it would ask (another build's access list, or a locked keychain: the Ask covers both), and
/// `None` the read said neither (an answer with no value, another error).
pub(super) fn silent_answer(read: Result<bool, i32>) -> Option<bool> {
    match read {
        Ok(true) | Err(ITEM_NOT_FOUND) => Some(true),
        Err(INTERACTION_NOT_ALLOWED) => Some(false),
        Ok(false) | Err(_) => None,
    }
}
