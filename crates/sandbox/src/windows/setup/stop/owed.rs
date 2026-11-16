//! `ResetOwed` (`record_values::RESET_OWED`): the resets a stop owes while it can't take the
//! set-up lock. A stop that kills without the lock (it couldn't reserve its number, or a mark's
//! wait ran out) still records every pool user it found, here, without the lock: it reads the
//! list, merges its users in, writes it and reads it back, up to [`OWE_TRIES`] times. Only the
//! stop task writes the value and it runs one instance at a time, so the read back guards
//! against a write that didn't land (a removal between, say), not against another writer. Such a
//! stop still never writes `StopSequence`.
//!
//! The next stop that holds the lock at its completion folds the list into `ResetSids` and
//! deletes it ([`fold`]), under the same hold and before it writes `StopSequence`, so a marker
//! that clears on that sequence finds every owed reset in `ResetSids`. `--remove` deletes it
//! with the set-up's values.

use super::{Failed, Stopper};
use crate::windows::setup::record_values::resets_with;
use crate::windows::sids::Sid;

/// How often [`owe`] tries.
pub(super) const OWE_TRIES: usize = 3;

/// Adds `users` to `ResetOwed` without the set-up lock, as the module says.
pub(super) fn owe(fs: &mut impl Stopper, users: &[Sid]) -> Result<(), Failed> {
    let mut why = String::new();
    for _ in 0..OWE_TRIES {
        let listed = match fs.read_owed() {
            Ok(listed) => listed,
            Err(failed) => {
                why = failed;
                continue;
            }
        };
        let all = resets_with(&listed, users);
        if all != listed
            && let Err(failed) = fs.write_owed(&all)
        {
            why = failed;
            continue;
        }
        match fs.read_owed() {
            Ok(back) if users.iter().all(|user| back.contains(user)) => return Ok(()),
            Ok(_) => "the read back missed a user".clone_into(&mut why),
            Err(failed) => why = failed,
        }
    }
    Err(format!(
        "ResetOwed wasn't written in {OWE_TRIES} tries: {why}"
    ))
}

/// With the set-up lock held and the record checked: `ResetOwed` added to `ResetSids`, then
/// deleted. Nothing to do when it lists no one.
pub(super) fn fold(fs: &mut impl Stopper) -> Result<(), Failed> {
    let owed = fs.read_owed()?;
    if owed.is_empty() {
        return Ok(());
    }
    fs.mark_for_reset(&owed)?;
    fs.write_owed(&[])
        .map_err(|why| format!("ResetOwed wasn't cleared once folded: {why}"))
}
