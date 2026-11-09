//! The pump of `Agents`' events (`AgentsBackend::pump`), which starts beside the keys' first
//! load (`start.rs`) and holds the sign-in events until it is over.

use std::collections::BTreeSet;

use clauseo_agents::AgentsEvent;
use clauseo_protocol::AgentId;
use futures::future::{Either, FutureExt as _, select};
use futures::{Stream, StreamExt as _};

use super::count::Tally;
use super::{AgentsBackend, pending};
use crate::bridge::lock;
use crate::open::open_url_unconfirmed;
use crate::setup::{events, leave};

/// Follows `Agents`' events until they end: installs out as `AgentInstallProgress` (and the
/// entry written on `Ready`), sign-ins through `Compose`, `OpenUrl` opened, notes logged.
/// Until the keys' first load is over, the sign-in events wait ([`waits_for_keys`]).
pub(super) async fn pump(backend: &AgentsBackend, source: impl Stream<Item = AgentsEvent>) {
    let mut source = std::pin::pin!(source);
    let mut checked = BTreeSet::new();
    let mut tally = Tally::default();
    // The pump starts beside the keys' first load (`start.rs`), so an install's `Ready`
    // asks for the status check at once. What the windows show of a sign-in is composed
    // with the key, so sign-in events wait here, in order, until the load is over. Events
    // that end before it drop what is held: the app is quitting.
    let mut held = Vec::new();
    let mut loaded = std::pin::pin!(backend.keys_loaded.wait().fuse());
    let mut over = false;
    loop {
        let event = if over {
            source.next().await
        } else {
            // The load's end first, so a load already over never holds an event.
            match select(loaded.as_mut(), source.next()).await {
                Either::Right((event, _)) => event,
                Either::Left(((), _)) => {
                    over = true;
                    for event in std::mem::take(&mut held) {
                        pumped(backend, event, &mut checked, &mut tally).await;
                    }
                    continue;
                }
            }
        };
        let Some(event) = event else {
            break;
        };
        if !over && waits_for_keys(&event) {
            held.push(event);
            continue;
        }
        pumped(backend, event, &mut checked, &mut tally).await;
    }
}

/// One event from `Agents`, taken in.
async fn pumped(
    backend: &AgentsBackend,
    event: AgentsEvent,
    checked: &mut BTreeSet<AgentId>,
    tally: &mut Tally,
) {
    match event {
        AgentsEvent::Install { agent, state } => {
            let now = backend.compose.monotonic();
            let tries = || backend.agents.install_tries(&agent);
            tally.install(&*backend.counts, &agent, &state, now, tries);
            backend.install_event(&agent, &state, checked).await;
            pending::follow_press(backend, &agent, &state);
        }
        AgentsEvent::Signin { agent, state } => {
            lock(&backend.rechecks).remove(&agent);
            tally.signin(&*backend.counts, &backend.compose, &agent, &state);
            leave::signin_event(backend, &agent, state).await;
        }
        AgentsEvent::OpenUrl { agent, url } => {
            if let Err(error) = open_url_unconfirmed(&*backend.opener, &url) {
                tracing::warn!(%agent, code = error.code().as_str(), "a sign-in page did not open");
            }
        }
        AgentsEvent::SigninNote { agent, note } => events::note(&agent, note),
        // A job that ended with no state (a status check that could not tell): an
        // `auth_required` re-check of it is over, so the next hold may ask again.
        AgentsEvent::SigninUnchanged { agent } => {
            lock(&backend.rechecks).remove(&agent);
        }
        _ => {}
    }
}

/// Whether `event` waits for the keys' first load in the pump: a sign-in state, whose card is
/// composed with the key, and a job's end with no state, which keeps its place after them.
fn waits_for_keys(event: &AgentsEvent) -> bool {
    matches!(
        event,
        AgentsEvent::Signin { .. } | AgentsEvent::SigninUnchanged { .. }
    )
}
