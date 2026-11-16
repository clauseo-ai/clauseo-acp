//! A matter on Clauseo's list inside the conversation's own matter, on the actor
//! (`agent::matter_fence::held`): a launch whose resolved fence holds one is refused
//! (`matter.holds_other_matter`), as is a prompt whose re-check finds one, and the lawyer gets a
//! notice naming the inner matter before the failure (`NoticeKind::MatterInside`).

use clauseo_protocol::{Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::matter_fence::held::{held, refusal};
use crate::error::Error;
use crate::log_file::Cause;

/// `run`'s launch refused when its fence holds a matter on the list inside the own matter,
/// the lawyer told which.
pub(crate) fn refuse(actor: &mut Actor, thread: ThreadId, run: &RunFolders) -> Result<(), Error> {
    let Some(other) = held(&run.data_deny.matters) else {
        return Ok(());
    };
    let error = refusal(other);
    tell(actor, thread, &error);
    Err(error)
}

/// Tells the lawyer which matter lies inside this one, when `error` is that refusal: the
/// notice names it as the list has it now. Ids only in the log.
pub(crate) fn tell(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let Error::Refused(refused) = error else {
        return;
    };
    let code = clauseo_protocol::ErrorCode::MatterHoldsOtherMatter;
    let Some(matter) = refused.matter().filter(|_| refused.code() == code) else {
        return;
    };
    let Some(name) = actor
        .matters
        .get(&matter)
        .map(|entry| entry.row.name.clone())
    else {
        return;
    };
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: format!(
            "{name} is inside this matter's folder. Move {name}'s folder out of it, or remove \
             one of the two matters from Clauseo."
        ),
        kind: Some(NoticeKind::MatterInside { matter, name }),
    };
    if let Err(error) = super::super::updates::commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the matter-inside notice was not logged");
    }
}
