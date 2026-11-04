//! `Agents` end to end, through its public API alone: a fixture snapshot of two agents made of
//! tiny tarballs, the local test registry under `registry.test`, the real Node and npm from
//! `PATH` (a test fails, rather than skips, when there is none), a `ManualTimer` the test
//! fires, and a clock that moves 250 ms per reading. No test sleeps or reaches the network.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "install/write.rs"]
mod write;

#[cfg(test)]
#[path = "install/support.rs"]
mod support;

#[cfg(test)]
#[path = "install/downloads.rs"]
mod downloads;

#[cfg(test)]
#[path = "install/layout.rs"]
mod layout;

#[cfg(test)]
#[path = "install/jobs.rs"]
mod jobs;

#[cfg(test)]
#[path = "install/firm.rs"]
mod firm;

#[cfg(test)]
#[path = "install/npm_file.rs"]
mod npm_file;
