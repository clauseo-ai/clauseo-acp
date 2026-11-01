//! [`Client`], the calls the usage reader makes, and [`Notifications`], what the app-server
//! sends unasked.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use futures::channel::{mpsc, oneshot};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Error;
use crate::types::{
    AccountRateLimitsUpdatedNotification, ClientInfo, GetAccountParams, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, InitializeCapabilities, InitializeParams,
    InitializeResponse,
};

/// The method names, as the schema's `ClientRequest`, `ClientNotification` and
/// `ServerNotification` give them.
pub(crate) mod method {
    pub(crate) const INITIALIZE: &str = "initialize";
    pub(crate) const INITIALIZED: &str = "initialized";
    pub(crate) const ACCOUNT_READ: &str = "account/read";
    pub(crate) const RATE_LIMITS_READ: &str = "account/rateLimits/read";
    pub(crate) const RATE_LIMITS_UPDATED: &str = "account/rateLimits/updated";
}

/// What a [`Client`] asks the driver to send.
pub(crate) enum Command {
    /// A request; the driver answers `reply` with the response's `result` or its error.
    Request {
        method: &'static str,
        params: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, Error>>,
    },
    /// A notification with no params.
    Notify { method: &'static str },
}

/// The calls the usage reader makes. Cheap to clone; each call awaits its own response, and
/// calls from clones may run at once. Nothing is sent until the [`Driver`](crate::Driver) runs.
#[derive(Debug, Clone)]
pub struct Client {
    commands: mpsc::UnboundedSender<Command>,
}

impl Client {
    pub(crate) fn new(commands: mpsc::UnboundedSender<Command>) -> Self {
        Self { commands }
    }

    /// Sends `initialize` with `info` and empty capabilities (so never `experimentalApi`), and
    /// once it is answered, the `initialized` notification.
    pub async fn initialize(&self, info: ClientInfo) -> Result<InitializeResponse, Error> {
        let params = InitializeParams {
            client_info: info,
            capabilities: Some(InitializeCapabilities::default()),
        };
        let response = self.call(method::INITIALIZE, &params).await?;
        self.send(Command::Notify {
            method: method::INITIALIZED,
        })?;
        Ok(response)
    }

    /// `account/read`, with empty params.
    pub async fn account(&self) -> Result<GetAccountResponse, Error> {
        self.call(method::ACCOUNT_READ, &GetAccountParams::default())
            .await
    }

    /// `account/rateLimits/read` with `params`.
    pub async fn rate_limits(
        &self,
        params: GetAccountRateLimitsParams,
    ) -> Result<GetAccountRateLimitsResponse, Error> {
        self.call(method::RATE_LIMITS_READ, &params).await
    }

    async fn call<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: &P,
    ) -> Result<T, Error> {
        let params = serde_json::to_value(params).map_err(|error| {
            Error::Protocol(format!("`{method}` params do not encode: {error}"))
        })?;
        let (reply, answer) = oneshot::channel();
        self.send(Command::Request {
            method,
            params,
            reply,
        })?;
        // A dropped reply means the driver ended without answering.
        let Ok(result) = answer.await else {
            return Err(Error::Closed);
        };
        serde_json::from_value(result?).map_err(|error| {
            Error::Protocol(format!("the `{method}` response does not decode: {error}"))
        })
    }

    fn send(&self, command: Command) -> Result<(), Error> {
        // The driver dropped its receiver: it has ended.
        if self.commands.unbounded_send(command).is_err() {
            return Err(Error::Closed);
        }
        Ok(())
    }
}

/// A notification from the app-server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    /// `account/rateLimits/updated`: a sparse update to the last rate limit snapshot.
    RateLimitsUpdated(Box<AccountRateLimitsUpdatedNotification>),
    /// Any other method; its params are dropped.
    Other {
        /// The notification's method.
        method: String,
    },
}

/// The app-server's notifications, in the order it sent them. Unbounded: the driver never
/// waits for the caller to read. Dropping it discards every later notification.
///
/// The stream ends when the [`Driver`](crate::Driver) does, and the driver ends once the last
/// [`Client`] is dropped, so a caller that keeps only the stream sees it end at once. Hold a
/// `Client` for as long as you want notifications.
#[derive(Debug)]
pub struct Notifications(pub(crate) mpsc::UnboundedReceiver<Notification>);

impl Stream for Notifications {
    type Item = Notification;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Notification>> {
        Pin::new(&mut self.get_mut().0).poll_next(cx)
    }
}
