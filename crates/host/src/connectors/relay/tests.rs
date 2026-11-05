//! The relay end to end: an agent-side client over loopback TCP, the relay, and S28's fake
//! connector (or a gated upstream where the order of answers matters).

mod accept;
mod auth;
mod caps;
mod forwarding;
mod handshake_refusals;
mod input_limits;
mod lifetime;
mod raw_replies;
mod spend;
mod support;
