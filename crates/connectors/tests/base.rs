//! The base's public pieces: pay links, out-of-credit results, the memory store, the loopback
//! listener, `Secret`, and the fake connector's own checks.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "base/classify.rs"]
mod classify;

#[cfg(test)]
#[path = "base/fake_api.rs"]
mod fake_api;

#[cfg(test)]
#[path = "base/fake_oauth.rs"]
mod fake_oauth;

#[cfg(test)]
#[path = "base/loopback.rs"]
mod loopback;

#[cfg(test)]
#[path = "base/memory_store.rs"]
mod memory_store;

#[cfg(test)]
#[path = "base/pay_url.rs"]
mod pay_url;

#[cfg(test)]
#[path = "base/secret.rs"]
mod secret;

#[cfg(test)]
#[path = "base/support.rs"]
mod support;
