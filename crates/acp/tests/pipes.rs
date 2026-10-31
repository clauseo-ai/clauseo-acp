//! `clauseo-acp` through its public API against an agent the test plays line by line over
//! in-memory pipes, for what a scripted `fake_agent` cannot check: the exact JSON the client
//! writes, and what it does with a stdout read that fails.
//!
//! A test crate root resolves modules beside itself, so each submodule in `pipes/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "pipes/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "pipes/prompts.rs"]
mod prompts;

#[cfg(test)]
#[path = "pipes/requests.rs"]
mod requests;

#[cfg(test)]
#[path = "pipes/updates.rs"]
mod updates;

#[cfg(test)]
#[path = "pipes/stdout.rs"]
mod stdout;
