//! The host thread's loop: requests from handles and internal inputs merged into one stream,
//! taken one at a time until shutdown or abort, each followed by the updater's tick, the triage
//! tick and the app lists' flush. Moved out of `actor.rs` as it was; no behaviour of its own.

use std::pin::pin;

use futures::stream::{self, StreamExt};

use super::input::{Input, Request};
use super::{Actor, Exit, app_lists, shutdown, triage, update};

/// One item of the merged input stream.
enum Next {
    Request(Request),
    /// Every handle is gone: shut down.
    HandlesGone,
    Input(Input),
}

/// The loop: one input at a time until shutdown or abort.
pub(super) async fn main_loop(
    actor: &mut Actor,
    requests: async_channel::Receiver<Request>,
    internal: async_channel::Receiver<Input>,
) -> Exit {
    let requests = requests
        .map(Next::Request)
        .chain(stream::once(async { Next::HandlesGone }));
    let mut inputs = pin!(stream::select(requests, internal.map(Next::Input)));
    loop {
        if let Some(exit) = actor.settle() {
            return exit;
        }
        match inputs.next().await {
            Some(Next::Request(request)) => {
                actor.on_request(request);
                update::tick(actor);
                triage::tick(actor);
                app_lists::flush(actor);
            }
            // Every handle is gone (the internal channel never ends: the actor holds a sender).
            Some(Next::HandlesGone) | None => shutdown::begin_shutdown(actor, None),
            #[cfg(feature = "test-support")]
            Some(Next::Input(Input::Abort(reply))) => return Exit::Abort(reply),
            Some(Next::Input(input)) => {
                actor.on_input(input);
                update::tick(actor);
                triage::tick(actor);
                app_lists::flush(actor);
            }
        }
    }
}
