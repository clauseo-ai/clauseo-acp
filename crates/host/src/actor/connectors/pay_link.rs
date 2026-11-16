//! `OpenPayLink` (R24, PR6, from the app review of #505): the pay link the state holds, the
//! checked https `PayUrl` `AddCredits` got from the recharge API (`CaseLaw::pay_link`: never
//! out of credit's, which came from a tool result, nor any URL of the relay's, a tool result's
//! or the agent's), opened through the native
//! confirm (`ConnectorOpener::open_confirmed`, H11's `open_external` in desktop), never the
//! unconfirmed opener. The window names only the connector; no URL crosses from it.
//!
//! The confirm waits on the lawyer, so it runs on a thread of its own and the command is
//! answered when it ends. A thread that ends without answering (a panic) answers `internal`
//! from `Drop`, so the command never waits forever.

use std::sync::Arc;

use clauseo_connectors::PayUrl;
use clauseo_protocol::{CommandId, ErrorCode, HostError};

use super::super::Actor;
use super::ConnectorInput;
use super::jobs::Post;
use crate::connectors::ConnectorOpener;
use crate::error::Error;

/// The thread's name.
const THREAD_NAME: &str = "clauseo-pay-link";

/// Opens `url` for command `command` behind the confirm, off the actor.
pub(super) fn open(
    actor: &Actor,
    command: CommandId,
    opener: Arc<dyn ConnectorOpener>,
    url: PayUrl,
) -> Result<(), Error> {
    let post = Post(actor.post.clone());
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let mut answer = Answer {
                post,
                command,
                reply: Some(Err(HostError::new(
                    ErrorCode::Internal,
                    "the pay link's confirm ended without an answer",
                ))),
            };
            let reply = opener.open_confirmed(url.as_str());
            if let Err(error) = &reply {
                tracing::info!(connector = "case-law", code = ?error.code(), "the pay link did not open");
            }
            answer.reply = Some(reply);
        })
        .map(drop)
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))
}

/// Posts the reply when dropped, on every way out of the thread.
struct Answer {
    post: Post,
    command: CommandId,
    reply: Option<Result<(), HostError>>,
}

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            self.post.send(ConnectorInput::PayLinkOpened {
                command: self.command,
                reply,
            });
        }
    }
}
