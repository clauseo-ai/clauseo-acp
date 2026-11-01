//! [`Driver`]: the one future that reads the app-server's stdout and writes its stdin.
//!
//! It runs two loops at once, so a write that waits on a full pipe never stops the reading: the
//! write loop sends the clients' requests and notifications and the answers to server requests;
//! the read loop matches responses to calls by id and passes notifications on. They share the
//! table of calls awaiting a response.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll};

use futures::channel::{mpsc, oneshot};
use futures::future::Either;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::client::{Command, Notification, method};
use crate::line::{LineError, LineReader, MAX_LINE_BYTES};
use crate::types::AccountRateLimitsUpdatedNotification;

/// JSON-RPC's "method not found", the answer to every request from the server.
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;

/// The connection's work. The caller runs it on its own executor, alongside its calls. It ends
/// with `Ok(())` once every [`Client`](crate::Client) is dropped (it then closes the
/// app-server's stdin), or with an [`Error`]: `Closed` when stdout ends or a write fails, and
/// `Protocol` for a line that is not a JSON-RPC message or is over 8 MiB. Either way every call
/// still awaiting a response fails with that error.
#[must_use = "the caller must run the driver, or nothing reaches the app-server"]
pub struct Driver(Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>);

impl Future for Driver {
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().0.as_mut().poll(cx)
    }
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Driver")
    }
}

impl Driver {
    pub(crate) fn new<W, R>(
        writer: W,
        reader: R,
        commands: mpsc::UnboundedReceiver<Command>,
        notifications: mpsc::UnboundedSender<Notification>,
    ) -> Self
    where
        W: AsyncWrite + Send + Unpin + 'static,
        R: AsyncRead + Send + Unpin + 'static,
    {
        Self(Box::pin(run(writer, reader, commands, notifications)))
    }
}

type Reply = oneshot::Sender<Result<serde_json::Value, Error>>;

/// The calls awaiting a response, by request id.
#[derive(Default)]
struct Pending {
    next_id: u64,
    calls: BTreeMap<u64, Reply>,
}

fn lock(pending: &Mutex<Pending>) -> std::sync::MutexGuard<'_, Pending> {
    // No code panics while holding the lock, and the table stays whole if one did.
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn run<W, R>(
    writer: W,
    reader: R,
    commands: mpsc::UnboundedReceiver<Command>,
    notifications: mpsc::UnboundedSender<Notification>,
) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    let pending = Mutex::new(Pending::default());
    let (answers, answer_receiver) = mpsc::unbounded();
    let reading = pin!(read_loop(reader, &pending, notifications, answers));
    let writing = pin!(write_loop(writer, &pending, commands, answer_receiver));
    let error = match futures::future::select(reading, writing).await {
        Either::Left((error, _)) | Either::Right((Err(error), _)) => error,
        Either::Right((Ok(()), _)) => return Ok(()),
    };
    let calls = std::mem::take(&mut lock(&pending).calls);
    for reply in calls.into_values() {
        // A caller that dropped its call's future no longer wants the answer.
        drop(reply.send(Err(error.clone())));
    }
    Err(error)
}

/// Writes requests, notifications and answers to server requests, one line each. Returns
/// `Ok(())` once every client is dropped, after closing the writer.
async fn write_loop<W: AsyncWrite + Unpin>(
    mut writer: W,
    pending: &Mutex<Pending>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut answers: mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<(), Error> {
    loop {
        let line = futures::select_biased! {
            answer = answers.select_next_some() => answer,
            command = commands.next().fuse() => match command {
                Some(command) => encode(pending, command)?,
                None => break,
            },
        };
        write_line(&mut writer, &line).await?;
    }
    if let Err(error) = writer.close().await {
        tracing::debug!(%error, "closing the app-server's stdin failed");
    }
    Ok(())
}

/// The line for `command`, a request registered in `pending` before it is written.
fn encode(pending: &Mutex<Pending>, command: Command) -> Result<Vec<u8>, Error> {
    let encoded = match command {
        Command::Request {
            method,
            params,
            reply,
        } => {
            let mut pending = lock(pending);
            let id = pending.next_id;
            pending.next_id += 1;
            pending.calls.insert(id, reply);
            serde_json::to_vec(&OutgoingRequest {
                id,
                method,
                params: &params,
            })
        }
        Command::Notify { method } => serde_json::to_vec(&OutgoingNotification { method }),
    };
    let mut line =
        encoded.map_err(|error| Error::Protocol(format!("a message does not encode: {error}")))?;
    line.push(b'\n');
    Ok(line)
}

async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, line: &[u8]) -> Result<(), Error> {
    let written = match writer.write_all(line).await {
        Ok(()) => writer.flush().await,
        Err(error) => Err(error),
    };
    written.map_err(|error| {
        tracing::debug!(%error, "writing to the app-server's stdin failed");
        Error::Closed
    })
}

/// Reads stdout until it ends or breaks the protocol, and returns why it stopped.
async fn read_loop<R: AsyncRead + Unpin>(
    reader: R,
    pending: &Mutex<Pending>,
    notifications: mpsc::UnboundedSender<Notification>,
    answers: mpsc::UnboundedSender<Vec<u8>>,
) -> Error {
    let mut lines = LineReader::new(reader);
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => return Error::Closed,
            Err(LineError::TooLong) => {
                return Error::Protocol(format!(
                    "the app-server sent a line over {MAX_LINE_BYTES} bytes"
                ));
            }
            Err(LineError::Io(error)) => {
                tracing::debug!(%error, "reading the app-server's stdout failed");
                return Error::Closed;
            }
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let message = match serde_json::from_slice::<Incoming>(&line) {
            Ok(message) => message,
            Err(error) => {
                return Error::Protocol(format!(
                    "the app-server sent a line that is not JSON-RPC: {error}"
                ));
            }
        };
        match message.kind() {
            Some(Kind::Response { id, result }) => answer(pending, &id, Ok(result)),
            Some(Kind::Error { id, error }) => answer(pending, &id, Err(error)),
            Some(Kind::Notification { method, params }) => {
                notify(&notifications, method, params);
            }
            Some(Kind::Request { id, method }) => {
                tracing::debug!(method, "refused a request from the app-server");
                match refusal(id, &method) {
                    // The write loop has ended only if the driver is ending too.
                    Ok(line) => drop(answers.unbounded_send(line)),
                    Err(error) => return error,
                }
            }
            None => {
                return Error::Protocol(
                    "the app-server sent JSON that is not a JSON-RPC message".to_owned(),
                );
            }
        }
    }
}

/// Hands a response to the call awaiting it.
fn answer(
    pending: &Mutex<Pending>,
    id: &serde_json::Value,
    result: Result<serde_json::Value, Error>,
) {
    let reply = id.as_u64().and_then(|id| lock(pending).calls.remove(&id));
    if let Some(reply) = reply {
        // A caller that dropped its call's future no longer wants the answer.
        drop(reply.send(result));
    } else {
        tracing::debug!(%id, "skipped a response to no pending call");
    }
}

fn notify(
    notifications: &mpsc::UnboundedSender<Notification>,
    method: String,
    params: Option<serde_json::Value>,
) {
    let notification = if method == method::RATE_LIMITS_UPDATED {
        let params = params.unwrap_or(serde_json::Value::Null);
        match serde_json::from_value::<AccountRateLimitsUpdatedNotification>(params) {
            Ok(update) => Notification::RateLimitsUpdated(Box::new(update)),
            Err(error) => {
                tracing::debug!(method, %error, "skipped a notification whose params do not decode");
                return;
            }
        }
    } else {
        tracing::debug!(method, "a notification this crate does not decode");
        Notification::Other { method }
    };
    // A caller that dropped the stream discards every later notification.
    drop(notifications.unbounded_send(notification));
}

/// The -32601 answer to a request from the server.
fn refusal(id: serde_json::Value, method: &str) -> Result<Vec<u8>, Error> {
    let answer = OutgoingError {
        id,
        error: ErrorBody {
            code: METHOD_NOT_FOUND,
            message: format!("method not found: {method}"),
        },
    };
    let mut line = serde_json::to_vec(&answer)
        .map_err(|error| Error::Protocol(format!("a message does not encode: {error}")))?;
    line.push(b'\n');
    Ok(line)
}

/// A request as written: no `jsonrpc` member (see the crate contract).
#[derive(Serialize)]
struct OutgoingRequest<'a> {
    id: u64,
    method: &'a str,
    params: &'a serde_json::Value,
}

/// A notification as written: the only one, `initialized`, has no params.
#[derive(Serialize)]
struct OutgoingNotification<'a> {
    method: &'a str,
}

#[derive(Serialize)]
struct OutgoingError {
    id: serde_json::Value,
    error: ErrorBody,
}

#[derive(Serialize, Deserialize)]
struct ErrorBody {
    code: i64,
    message: String,
}

/// Any message the app-server sends, with or without `"jsonrpc": "2.0"`; unknown members,
/// `jsonrpc` among them, are ignored.
#[derive(Deserialize)]
struct Incoming {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
    /// `Some(Null)` when the member is `null`, `None` when it is missing.
    #[serde(default, deserialize_with = "present")]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<ErrorBody>,
}

fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
}

enum Kind {
    Request {
        id: serde_json::Value,
        method: String,
    },
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
    Response {
        id: serde_json::Value,
        result: serde_json::Value,
    },
    Error {
        id: serde_json::Value,
        error: Error,
    },
}

impl Incoming {
    fn kind(self) -> Option<Kind> {
        match (self.id, self.method, self.result, self.error) {
            (Some(id), Some(method), None, None) => Some(Kind::Request { id, method }),
            (None, Some(method), None, None) => Some(Kind::Notification {
                method,
                params: self.params,
            }),
            (Some(id), None, Some(result), None) => Some(Kind::Response { id, result }),
            (Some(id), None, None, Some(error)) => Some(Kind::Error {
                id,
                error: Error::Rpc {
                    code: error.code,
                    message: error.message,
                },
            }),
            _ => None,
        }
    }
}
