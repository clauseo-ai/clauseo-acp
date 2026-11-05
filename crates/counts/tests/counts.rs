//! The client of `clauseo-counts`: cells, the sender, the first act and policy, on a manual
//! clock, a scratch data folder and a fake `Upload`.
//!
//! A test crate root resolves modules beside itself, so each submodule in `counts/` names its
//! path. Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#![cfg(feature = "client")]

#[cfg(test)]
#[path = "counts/support.rs"]
mod support;
#[cfg(test)]
#[path = "counts/write.rs"]
mod write;

#[cfg(test)]
#[path = "counts/cells.rs"]
mod cells;
#[cfg(test)]
#[path = "counts/first_act.rs"]
mod first_act;
#[cfg(test)]
#[path = "counts/off_on.rs"]
mod off_on;
#[cfg(test)]
#[path = "counts/policy.rs"]
mod policy;
#[cfg(test)]
#[path = "counts/sender.rs"]
mod sender;
