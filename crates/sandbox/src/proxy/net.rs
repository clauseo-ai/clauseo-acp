//! The proxy's only reach into the network: name resolution and one connect to one checked
//! address. A seam, so tests map documentation-range addresses to in-process servers and
//! record every address dialled.

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};

use async_io::Async;
use futures::future::BoxFuture;

/// Resolves names and connects to addresses.
pub(crate) trait Network: Send + Sync {
    /// Every address `host` resolves to, with `port`. Runs off the I/O thread.
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>>;
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

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>> {
        Box::pin(Async::<TcpStream>::connect(addr))
    }
}
