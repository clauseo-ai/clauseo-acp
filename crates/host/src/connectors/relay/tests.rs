//! The relay end to end: an agent-side client over loopback TCP, the relay, and S28's fake
//! connector (or a gated upstream where the order of answers matters).

mod accept;
mod auth;
mod cache;
mod caps;
mod forwarding;
mod handshake_refusals;
mod input_limits;
mod lifetime;
mod local_answers;
mod raw_replies;
mod recharge_reply;
mod spend;
mod support;
mod warm_up;
