//! The binary over real pipes, plus the checks on the bundled scenarios and the manifest.
//!
//! A test crate root resolves modules beside itself, so each submodule in `stdio/` names its path.
//! Each carries `#[cfg(test)]` so clippy treats its helpers as test code.

#[cfg(test)]
#[path = "stdio/harness.rs"]
mod harness;

#[cfg(test)]
#[path = "stdio/cli.rs"]
mod cli;
#[cfg(test)]
#[path = "stdio/files.rs"]
mod files;
#[cfg(test)]
#[path = "stdio/handshake.rs"]
mod handshake;
#[cfg(test)]
#[path = "stdio/manifest.rs"]
mod manifest;
#[cfg(test)]
#[path = "stdio/permission.rs"]
mod permission;
#[cfg(test)]
#[path = "stdio/process.rs"]
mod process;
#[cfg(test)]
#[path = "stdio/stream.rs"]
mod stream;
