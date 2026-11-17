//! The proxy over in-process sockets: a test network maps documentation-range addresses to
//! servers on 127.0.0.1 and records each address dialled, so nothing leaves the machine while
//! the loopback refusal stays real.

mod allowed;
mod blocked;
mod browser;
mod connect;
mod failed;
mod harness;
mod http;
mod limits;
mod local_net;
mod open;
mod open_name_only;
mod open_private;
mod open_probe;
mod open_unloaded;
mod open_upstream;
mod private;
pub(super) mod test_net;
mod unreachable;
mod upstream;
