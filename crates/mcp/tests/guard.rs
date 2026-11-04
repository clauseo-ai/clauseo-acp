//! The built `clauseo-mcp` over real pipes, with the stub children in `examples/` standing in for
//! agent-browser, plus the dependency check.
//!
//! A test crate root resolves modules beside itself, so each submodule in `guard/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code. No sleeps or timeouts in
//! the tests: every wait is a blocking read, and nextest's slow-timeout ends a hung test. Two
//! tests wait out the guard's own limits (10 s for the close reply, 5 s for the child's exit).

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
#[cfg(all(test, windows))]
#[path = "guard/tree.rs"]
mod tree;
#[cfg(test)]
#[path = "guard/usage.rs"]
mod usage;
