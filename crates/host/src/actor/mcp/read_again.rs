//! `KeychainReadAgain`'s part for the lawyer's MCP servers: once the lawyer allowed the keychain,
//! each value a launch left out because the keychain refused its read (`McpOnActor::refused`,
//! never one found missing) is read again on a keychain thread. A value that reads is no longer
//! shown unsaved; one now found missing stays unsaved, no longer refused; one refused again
//! stays as it was. Then the view goes out (`McpToolsChanged`, and `AddonsChanged` when the
//! list moved). The value read is zeroed at once: each launch reads its own.
//!
//! The reads run on a thread of their own, beside the commands' keychain queue: a read changes
//! nothing in the keychain, and a save or a removal that lands meanwhile forgets the value
//! first, so its answer is dropped. A thread that ends without answering changes nothing.

use super::{McpInput, THREAD_NAME, view};
use crate::actor::Actor;
use crate::actor::input::Input;

/// One refused value read again: the server, the variable or header, and what the read found.
#[derive(Debug)]
pub(crate) struct Read {
    server: String,
    var: String,
    found: Found,
}

/// What a read found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// The item is there.
    There,
    /// No item.
    Missing,
    /// Refused again.
    Refused,
}

/// `KeychainReadAgain`: reads the refused values again, off the actor. Nothing to read, or no
/// keychain, does nothing.
pub(in crate::actor) fn ask(actor: &mut Actor) {
    let Some(store) = actor.wiring.mcp_secrets.clone() else {
        return;
    };
    let wanted: Vec<((String, String), String)> = actor
        .mcp
        .refused
        .iter()
        .map(|(key, account)| (key.clone(), account.clone()))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let post = actor.post.clone();
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let read = wanted
                .into_iter()
                .map(|((server, var), account)| {
                    let found = match store.get(&account) {
                        Ok(Some(bytes)) => {
                            drop(zeroize::Zeroizing::new(bytes));
                            Found::There
                        }
                        Ok(None) => Found::Missing,
                        Err(error) => {
                            tracing::warn!(%error, server = server.as_str(), "an MCP server's keychain value is still refused");
                            Found::Refused
                        }
                    };
                    Read { server, var, found }
                })
                .collect();
            let _gone = post.try_send(Input::Mcp(Box::new(McpInput::ReadAgain(read))));
        });
    if let Err(error) = started {
        tracing::warn!(%error, "the MCP values were not read again");
    }
}

/// The read ended: what it found goes into the view.
pub(super) fn on_read(actor: &mut Actor, read: Vec<Read>) {
    for Read { server, var, found } in read {
        let key = (server, var);
        // A save or a removal meanwhile already forgot it.
        if !actor.mcp.refused.contains_key(&key) {
            continue;
        }
        match found {
            Found::There => {
                actor.mcp.refused.remove(&key);
                actor.mcp.missing.remove(&key);
            }
            Found::Missing => {
                actor.mcp.refused.remove(&key);
            }
            Found::Refused => {}
        }
    }
    view::publish(actor);
}
