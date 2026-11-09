//! macOS, tier A: the Seatbelt profile ([`SeatbeltProfile`], built on every target so its
//! snapshots run everywhere), the launcher that runs an agent under it through
//! `/usr/bin/sandbox-exec` ([`SeatbeltLauncher`]), and the one host-wide watcher that turns the
//! unified log's Sandbox lines into [`Denial`](crate::Denial)s, and the agent's process cap
//! (`macos/process_cap.rs`), and the session sweep that finds and ends every process of a
//! launch wherever it went (`sweep`, macOS only). [`browser`] is the browser stack's own
//! profile, which runs outside the jail.

pub mod browser;
#[cfg(target_os = "macos")]
mod launcher;
#[cfg(test)]
mod no_bind;
#[cfg(target_os = "macos")]
mod process_cap;
mod profile;
#[cfg(target_os = "macos")]
pub mod sweep;
#[cfg(target_os = "macos")]
mod watch;

#[cfg(target_os = "macos")]
pub use launcher::SeatbeltLauncher;
#[cfg(target_os = "macos")]
pub use process_cap::{ProcessCapNotice, ProcessCapSession};
pub use profile::{INSTALLED_BROWSERS, SeatbeltProfile};
