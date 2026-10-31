//! The store's tests, one binary: each module tests one part at the interface.
//!
//! A test crate root resolves modules beside itself, so each submodule in `store/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "store/common.rs"]
mod common;
#[cfg(test)]
#[path = "store/write.rs"]
mod write;

#[cfg(test)]
#[path = "store/close.rs"]
mod close;
#[cfg(test)]
#[path = "store/edges.rs"]
mod edges;
#[cfg(test)]
#[path = "store/errors.rs"]
mod errors;
#[cfg(test)]
#[path = "store/files.rs"]
mod files;
#[cfg(test)]
#[path = "store/golden.rs"]
mod golden;
#[cfg(test)]
#[path = "store/group.rs"]
mod group;
#[cfg(test)]
#[path = "store/log.rs"]
mod log;
#[cfg(test)]
#[path = "store/log_model.rs"]
mod log_model;
#[cfg(test)]
#[path = "store/reopen.rs"]
mod reopen;
#[cfg(test)]
#[path = "store/rows.rs"]
mod rows;
