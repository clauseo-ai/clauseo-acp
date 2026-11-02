//! The built `clauseo-mcp` over real pipes, with the stub children in `examples/` standing in for
//! agent-browser, plus the dependency check.
//!
//! A test crate root resolves modules beside itself, so each submodule in `guard/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code. No sleeps or timeouts:
//! every wait is a blocking read, and nextest's slow-timeout ends a hung test.

#[cfg(test)]
#[path = "guard/support.rs"]
mod support;
#[cfg(test)]
#[path = "guard/write.rs"]
mod write;

#[cfg(test)]
#[path = "guard/deps.rs"]
mod deps;
#[cfg(test)]
#[path = "guard/env.rs"]
mod env;
#[cfg(test)]
#[path = "guard/filter.rs"]
mod filter;
#[cfg(test)]
#[path = "guard/usage.rs"]
mod usage;
