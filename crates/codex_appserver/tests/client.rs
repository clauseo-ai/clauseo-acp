//! The client over in-memory pipes, where each test plays the app-server line by line and
//! answers with the fixtures in `tests/fixtures/`.
//!
//! A test crate root resolves modules beside itself, so each submodule in `client/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "client/peer.rs"]
mod peer;

#[cfg(test)]
#[path = "client/calls.rs"]
mod calls;
