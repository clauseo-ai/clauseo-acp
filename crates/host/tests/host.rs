//! The host's integration tests, one binary: each module drives real `fake_agent` processes
//! through one part of the host at its public interface (`HostHandle`, the subscriptions, the
//! files in the matter folder). Every test owns a folder under the temp folder and moves time
//! only with `ManualClock`.
//!
//! A test crate root resolves modules beside itself, so each submodule in `host/` names its
//! path. Each carries `#[cfg(test)]` so clippy treats its helpers as test code.
//!
//! `fake_agent` is built only in a workspace run:
//! `cargo nextest run --workspace -E 'package(clauseo-host)'`.

#[cfg(test)]
#[path = "host/observe.rs"]
mod observe;
#[cfg(test)]
#[path = "host/support.rs"]
mod support;
#[cfg(test)]
#[path = "host/write.rs"]
mod write;

#[cfg(test)]
#[path = "host/blobs.rs"]
mod blobs;
#[cfg(test)]
#[path = "host/cancel.rs"]
mod cancel;
#[cfg(test)]
#[path = "host/commands.rs"]
mod commands;
#[cfg(test)]
#[path = "host/limits.rs"]
mod limits;
#[cfg(test)]
#[path = "host/permissions.rs"]
mod permissions;
#[cfg(test)]
#[path = "host/restart.rs"]
mod restart;
#[cfg(test)]
#[path = "host/subscribe.rs"]
mod subscribe;
#[cfg(test)]
#[path = "host/turn.rs"]
mod turn;
#[cfg(test)]
#[path = "host/writer.rs"]
mod writer;
