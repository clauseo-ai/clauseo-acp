//! The refresh's heal of the group's deny (step 3, `admin_copy/heal.rs` says why): with the
//! set-up lock held and the record read, a `GroupSid` that is a SID has the admin folder and
//! copy given its deny when both are locked exactly and either lacks only that. It runs before
//! anything else the run may refuse on, so a run that copies nothing still heals; the launcher
//! asks the task for it and waits. A failure is logged and the run goes on: the copy stays as it
//! was, and the launcher refuses it as before.

use super::{RefreshRecord, Refresher};
use crate::windows::sids::Sid;

/// The heal, as the module says. A `GroupSid` that isn't a SID is left for step 3's refusal.
pub(super) fn heal(fs: &mut impl Refresher, record: &RefreshRecord) {
    let Some(Ok(group)) = record.group_sid.as_deref().map(Sid::parse) else {
        return;
    };
    match fs.deny_group(&group) {
        Ok(true) => fs.log(
            false,
            "the sandbox's admin copy now denies the sandbox group",
        ),
        Ok(false) => {}
        Err(why) => fs.log(
            true,
            &format!("the sandbox's admin copy couldn't be given the sandbox group's deny: {why}"),
        ),
    }
}
