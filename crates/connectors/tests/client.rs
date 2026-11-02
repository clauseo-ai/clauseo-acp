//! The client against the fake connector: discovery, sign-in, the token store and refresh, the
//! API key, sign-out, liveness, the account, recharge and `mcp_post`.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "client/calls.rs"]
mod calls;

#[cfg(test)]
#[path = "client/credentials.rs"]
mod credentials;

#[cfg(test)]
#[path = "client/discovery.rs"]
mod discovery;

#[cfg(test)]
#[path = "client/mcp.rs"]
mod mcp;

#[cfg(test)]
#[path = "client/refresh.rs"]
mod refresh;

#[cfg(test)]
#[path = "client/signin.rs"]
mod signin;

#[cfg(test)]
#[path = "client/support.rs"]
mod support;
