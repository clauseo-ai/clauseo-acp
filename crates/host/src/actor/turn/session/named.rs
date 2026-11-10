//! The agent's name, as `initialize` gave it, logged once per name (`AgentNamed`).

use clauseo_protocol::{AgentNamed, CompactString, StreamId, ThreadId};

use super::super::super::Actor;
use super::super::Then;
use super::super::out::Out;
use crate::error::Error;
use crate::log_file::Cause;

/// The name the thread's header holds: from the loaded stream, else from the header the actor
/// kept when the stream was unloaded.
fn held_name(actor: &Actor, thread: ThreadId) -> Option<CompactString> {
    let entry = actor.threads.get(&thread)?;
    let header = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(entry.header.as_ref())?;
    header.agent_name.clone()
}

/// Logs `AgentNamed` for `thread` when `name` is set and differs from the name its header holds,
/// so a restart with the same agent logs nothing. A push or send refused at once is logged and
/// the session goes on without the name. A store write that fails later is a store failure like
/// any other commit's: `turn.rs` fails the launching turn with it.
pub(super) fn log_agent_name(actor: &mut Actor, thread: ThreadId, name: Option<CompactString>) {
    let Some(name) = name else {
        return;
    };
    if held_name(actor, thread).as_ref() == Some(&name) {
        return;
    }
    let logged = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        out.push(
            actor,
            AgentNamed {
                agent_name: name.clone(),
            },
        )?;
        out.send(actor, Then::Nothing)
    })();
    match logged {
        Ok(()) => {
            // An unloaded stream folds nothing, so the header the actor kept takes the name here:
            // a second start while it is unloaded then logs nothing.
            if let Some(header) = actor
                .threads
                .get_mut(&thread)
                .and_then(|entry| entry.header.as_mut())
            {
                header.agent_name = Some(name);
            }
        }
        Err(error) => {
            tracing::warn!(%thread, error = %Cause(&error), "the agent's name was not logged");
        }
    }
}
