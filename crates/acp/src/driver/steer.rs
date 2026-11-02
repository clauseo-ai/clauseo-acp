//! `steer`: the host's message into a running turn, sent as `_session/steering` with the text as
//! a prompt. It goes out while the turn's `session/prompt` still waits: nothing here waits for
//! that prompt or changes the session's origin, so the turn's later updates stay the running
//! turn's.

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::{Agent, ConnectionTo, UntypedMessage};
use clauseo_protocol::{AgentSessionId, ContentBlock};

use super::{answer, known, send_with};
use crate::Error;
use crate::client::Reply;
use crate::map;
use crate::state::Shared;
use crate::steering;
use crate::types::SteerResult;

/// The steering extension method (`_session/steering`).
const STEERING_METHOD: &str = "_session/steering";

/// Sends the steer; the answer is the [`SteerResult`]. An error answer is `Failed`; a transport
/// error (the agent is gone) stays an error.
pub(crate) fn steer(
    cx: &ConnectionTo<Agent>,
    shared: &Arc<Shared>,
    session: &AgentSessionId,
    content: Vec<ContentBlock>,
    reply: Reply<SteerResult>,
) {
    let (supported, capabilities) = {
        let state = shared.state();
        (
            steering::supported(state.steering),
            state.prompt_capabilities,
        )
    };
    if !supported {
        return answer(reply, Err(Error::SteerUnsupported));
    }
    if !known(shared, session) {
        return answer(reply, Err(Error::UnknownSession));
    }
    let blocks = content
        .into_iter()
        .map(|block| map::prompt_block(block, capabilities))
        .collect::<Result<Vec<_>, Error>>();
    let blocks = match blocks {
        Ok(blocks) => blocks,
        Err(error) => return answer(reply, Err(error)),
    };
    // The same parameters as `session/prompt`, with the steering `_meta`.
    let mut request = acp::PromptRequest::new(session.as_str().to_owned(), blocks);
    request.meta = Some(steering::request_meta());
    let message = match UntypedMessage::new(STEERING_METHOD, request) {
        Ok(message) => message,
        Err(error) => return answer(reply, Err(Error::Protocol(error.to_string()))),
    };
    send_with(cx, shared, message, reply, |result| async move {
        match result {
            Ok(value) => Ok(steering::outcome(&value)),
            // The agent refused the steer with an error response.
            Err(Error::Rpc { .. } | Error::AuthRequired { .. }) => Ok(SteerResult::Failed),
            Err(error) => Err(error),
        }
    });
}
