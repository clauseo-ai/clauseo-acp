//! The jail on this Mac: `fake_agent` and system tools run through `SeatbeltLauncher` against
//! temp folders laid out like a data dir, two matters, a login home and a Claude store. Each
//! "fails" check asserts the refusal (a non-zero exit and, where the tool prints it, "Operation
//! not permitted"), never a timeout. macOS only; `fake_agent` must be built first
//! (`cargo build -p clauseo-fake_agent`).
//!
//! A test crate root resolves modules beside itself, so each submodule names its path.

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/support.rs"]
mod support;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/agent.rs"]
mod agent;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/files.rs"]
mod files;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/net.rs"]
mod net;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/git.rs"]
mod git;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/tree.rs"]
mod tree;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/watcher.rs"]
mod watcher;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/links.rs"]
mod links;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/devices.rs"]
mod devices;

#[cfg(test)]
#[cfg(target_os = "macos")]
#[path = "jail/env.rs"]
mod env;
