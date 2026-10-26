//! `clauseo-acp` against the real `fake_agent` binary over stdio, one test per scenario case.
//!
//! A test crate root resolves modules beside itself, so each submodule in `stdio/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "stdio/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "stdio/handshake.rs"]
mod handshake;

#[cfg(test)]
#[path = "stdio/turns.rs"]
mod turns;
