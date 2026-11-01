//! The scripted app-server: in-memory pipes, the test's side of them, the fixtures, and a
//! subscriber that keeps debug lines.

use std::fmt::Write as _;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use clauseo_codex_appserver::{
    Client, Driver, GetAccountRateLimitsParams, Notification, Notifications, connect,
};
use futures::channel::mpsc;
use futures::{AsyncRead, AsyncWrite, FutureExt as _, StreamExt};
use serde_json::{Value, json};

/// The longest line the client takes, as the crate caps it.
pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// The app-server's stdin: each write arrives as one message (the client writes a line at once).
pub(crate) struct Stdin(mpsc::UnboundedSender<Vec<u8>>);

impl AsyncWrite for Stdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        drop(self.0.unbounded_send(buf.to_vec()));
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.close_channel();
        Poll::Ready(Ok(()))
    }
}

/// The app-server's stdout: the chunks the test sends, read in order.
pub(crate) struct Stdout {
    chunks: mpsc::UnboundedReceiver<Vec<u8>>,
    /// What is left of the chunk being read.
    rest: Vec<u8>,
}

impl AsyncRead for Stdout {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        while this.rest.is_empty() {
            match futures::ready!(this.chunks.poll_next_unpin(cx)) {
                Some(chunk) => this.rest = chunk,
                None => return Poll::Ready(Ok(0)),
            }
        }
        let count = this.rest.len().min(buf.len());
        buf[..count].copy_from_slice(&this.rest[..count]);
        this.rest.drain(..count);
        Poll::Ready(Ok(count))
    }
}

/// The test's side of the app-server: what the client wrote, and what to write back.
pub(crate) struct Server {
    written: mpsc::UnboundedReceiver<Vec<u8>>,
    stdout: mpsc::UnboundedSender<Vec<u8>>,
}

impl Server {
    /// The next message the client wrote, which must be one whole line.
    pub(crate) async fn read(&mut self) -> Value {
        let line = self.written.next().await.expect("the client closed stdin");
        assert_eq!(line.last(), Some(&b'\n'));
        serde_json::from_slice(&line).unwrap()
    }

    /// The next message, which must be a request for `method`; it never carries `jsonrpc`.
    pub(crate) async fn request(&mut self, method: &str) -> Value {
        let request = self.read().await;
        assert_eq!(request["method"], method, "{request}");
        assert!(request["id"].is_u64(), "{request}");
        assert!(request.get("jsonrpc").is_none(), "{request}");
        request
    }

    /// Asserts the client writes nothing more and closes stdin.
    pub(crate) async fn read_end(&mut self) {
        if let Some(line) = self.written.next().await {
            panic!(
                "expected stdin to close, got {}",
                String::from_utf8_lossy(&line)
            );
        }
    }

    pub(crate) fn send_raw(&self, bytes: &[u8]) {
        self.stdout.unbounded_send(bytes.to_vec()).unwrap();
    }

    pub(crate) fn send(&self, message: &Value) {
        let mut line = serde_json::to_vec(message).unwrap();
        line.push(b'\n');
        self.send_raw(&line);
    }

    pub(crate) fn respond(&self, request: &Value, result: &Value) {
        self.send(&json!({"id": request["id"], "result": result}));
    }

    /// Ends stdout, as an exiting app-server does.
    pub(crate) fn exit(self) {
        drop(self.stdout);
    }
}

pub(crate) fn pipes() -> (Client, Notifications, Driver, Server) {
    let (written_tx, written) = mpsc::unbounded();
    let (stdout, chunks) = mpsc::unbounded();
    let reader = Stdout {
        chunks,
        rest: Vec::new(),
    };
    let (client, notifications, driver) = connect(Stdin(written_tx), reader);
    (client, notifications, driver, Server { written, stdout })
}

pub(crate) fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

pub(crate) fn reader_params() -> GetAccountRateLimitsParams {
    GetAccountRateLimitsParams {
        exclude_reset_credit_details: Some(true),
        ..GetAccountRateLimitsParams::default()
    }
}

/// Every notification the stream holds now.
pub(crate) fn drain(notifications: &mut Notifications) -> Vec<Notification> {
    std::iter::from_fn(|| notifications.next().now_or_never().flatten()).collect()
}

/// The debug events logged while `run` runs, each as its fields' text.
pub(crate) fn debug_lines(run: impl FnOnce()) -> Vec<String> {
    let capture = Capture(Arc::default());
    let lines = Arc::clone(&capture.0);
    tracing::subscriber::with_default(capture, run);
    let lines = lines.lock().unwrap();
    lines.clone()
}

/// A subscriber that keeps each debug event's fields.
pub(crate) struct Capture(Arc<Mutex<Vec<String>>>);

pub(crate) struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).unwrap();
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::DEBUG {
            let mut fields = Fields(String::new());
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}
