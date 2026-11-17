//! The proxy's only reach into the network: name resolution and one connect to one checked
//! address. A seam, so tests map documentation-range addresses to in-process servers and
//! record every address dialled.

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::thread;

use async_io::Async;
use futures::channel::oneshot;
use futures::future::BoxFuture;

/// Resolves names and connects to addresses.
pub(crate) trait Network: Send + Sync {
    /// Every address `host` resolves to, with `port`. Runs off the I/O thread.
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>>;
    /// As [`resolve`](Network::resolve), on a thread of its own: the name-only path's probe
    /// (`name_only::Gate`), which lookups a page started must not hold up.
    fn resolve_alone(
        &self,
        host: &str,
        port: u16,
    ) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        self.resolve(host, port)
    }
    /// One connect to `addr`, which the caller has checked; never a name.
    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>>;
}

/// The OS's resolver (on `blocking`'s pool) and real sockets.
pub(crate) struct SystemNetwork;

impl Network for SystemNetwork {
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        let host = host.to_owned();
        Box::pin(blocking::unblock(move || {
            (host.as_str(), port)
                .to_socket_addrs()
                .map(Iterator::collect)
        }))
    }

    /// A thread per call, which a probe makes at most once every 10 s; `blocking`'s pool when no
    /// thread can be made.
    fn resolve_alone(
        &self,
        host: &str,
        port: u16,
    ) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        let (send, answer) = oneshot::channel();
        let owned = host.to_owned();
        let spawned = thread::Builder::new()
            .name("clauseo-dns-probe".to_owned())
            .spawn(move || {
                let found = (owned.as_str(), port)
                    .to_socket_addrs()
                    .map(Iterator::collect);
                let _gone = send.send(found);
            });
        if spawned.is_err() {
            return self.resolve(host, port);
        }
        Box::pin(async move {
            answer
                .await
                .unwrap_or_else(|_gone| Err(io::Error::other("the probe's thread ended")))
        })
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>> {
        Box::pin(Async::<TcpStream>::connect(addr))
    }
}
