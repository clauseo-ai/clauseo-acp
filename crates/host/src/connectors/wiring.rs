//! What the host takes to run the connector commands (R24, PR5): the app's [`Connectors`], the
//! sink the relays report to ([`ConnectorEvents`]), and the opener that shows the sign-in page
//! ([`ConnectorOpener`]). `HostConfig::with_connectors` takes them as one [`ConnectorsWiring`].
//!
//! The relays (`HostConfig::with_case_law_relays`) are built by the caller with the same
//! `Connectors`' `case-law` handle and with [`ConnectorEvents`] as their `events`; the host binds
//! the sink to its actor when it starts, so what a relay hears (a credential that ended, out of
//! credit, a paid result) reaches the connector state. Before the host starts, or with no
//! connectors wired, an event is dropped.

use std::fmt;
use std::sync::{Arc, OnceLock};

use clauseo_connectors::Connectors;

use super::{RelayEvent, RelayEvents};

/// Opens the host's own links in the lawyer's browser. Desktop passes `open_url_unconfirmed` and
/// H11's `open_external`; tests a stand-in that drives the fake connector's pages.
pub trait ConnectorOpener: Send + Sync {
    /// Opens `url` with no confirm: the host's own sign-in page, never a link from a window or an
    /// agent. An error is a short line with no URL in it.
    fn open(&self, url: &str) -> Result<(), String>;

    /// Opens `url` after the native confirm that names where it goes (H11's `open_external`):
    /// the connector's checked pay link (`OpenPayLink`). Blocks until the lawyer answers;
    /// declined, nothing opens and the error is `transport.unauthorized`. Called off the actor.
    ///
    /// # Errors
    ///
    /// The confirm's or the opener's refusal, with no URL in its message.
    fn open_confirmed(&self, url: &str) -> Result<(), clauseo_protocol::HostError>;
}

/// The connector commands' wiring (`HostConfig::with_connectors`).
#[derive(Clone)]
pub struct ConnectorsWiring {
    /// The app's connectors: the credential in the keychain, liveness and the account.
    pub connectors: Arc<Connectors>,
    /// The sink the relays were built with.
    pub events: Arc<ConnectorEvents>,
    /// Shows the sign-in page.
    pub opener: Arc<dyn ConnectorOpener>,
}

impl fmt::Debug for ConnectorsWiring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorsWiring")
            .field("connectors", &self.connectors)
            .finish_non_exhaustive()
    }
}

/// The function the actor binds into a [`ConnectorEvents`].
pub(crate) type EventSink = Box<dyn Fn(RelayEvent) + Send + Sync>;

/// The relays' events sink: built before the host, bound to the actor when it starts.
#[derive(Default)]
pub struct ConnectorEvents {
    sink: OnceLock<EventSink>,
}

impl fmt::Debug for ConnectorEvents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorEvents")
            .field("bound", &self.sink.get().is_some())
            .finish()
    }
}

impl ConnectorEvents {
    /// A sink bound to nothing yet.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Binds the sink, once; a second bind (a second host on the same sink) is ignored and
    /// logged.
    pub(crate) fn bind(&self, sink: EventSink) {
        if self.sink.set(sink).is_err() {
            tracing::warn!("the connector events were already bound to a host");
        }
    }
}

impl RelayEvents for ConnectorEvents {
    fn event(&self, event: RelayEvent) {
        if let Some(sink) = self.sink.get() {
            sink(event);
        } else {
            tracing::debug!("a relay event before the host started was dropped");
        }
    }
}
