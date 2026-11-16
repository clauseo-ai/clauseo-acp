//! The policy lock causes in the host log: one `warn` line per cause
//! (`clauseo_settings::LockReason`) when policy locked keys without setting them (a source that
//! cannot be read, a value that does not type, a misnamed section, a key twice in a
//! `policy.json`). A plain policy value is no cause and logs nothing. Each line holds the cause's
//! code, the source's kind, the section only when it is a plain word, and the place: no value
//! and no path (decision record 0047).
//!
//! Logged at start-up, and again only when the set of causes changes, so a reload of the same
//! broken policy (a settings watcher) writes nothing new: [`log`] keeps the set it last logged.

#[cfg(test)]
mod tests;

use clauseo_settings::{Effective, LockReason};

/// Logs each of `effective`'s lock causes that `logged` does not hold, then makes `logged`
/// the current set. The same set again logs nothing; a changed one logs only the new causes.
pub(in crate::actor) fn log(logged: &mut Vec<LockReason>, effective: &Effective) {
    let now = effective.lock_reasons();
    if now == logged.as_slice() {
        return;
    }
    for reason in now.iter().filter(|reason| !logged.contains(reason)) {
        tracing::warn!(
            cause = reason.cause.code(),
            source = reason.source.name(),
            section = reason.section.as_deref().unwrap_or("-"),
            place = reason.place,
            keys = reason.keys.len(),
            "a policy locked settings it does not set"
        );
    }
    *logged = now.to_vec();
}
