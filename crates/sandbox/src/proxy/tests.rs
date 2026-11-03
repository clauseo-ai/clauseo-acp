//! The proxy over in-process sockets: a test network maps documentation-range addresses to
//! servers on 127.0.0.1 and records each address dialled, so nothing leaves the machine while
//! the loopback refusal stays real.

mod connect;
mod harness;
mod http;
mod limits;
mod private;
mod upstream;
