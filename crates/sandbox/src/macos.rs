//! macOS, tier A: the Seatbelt profile ([`SeatbeltProfile`], built on every target so its
//! snapshots run everywhere), the launcher that runs an agent under it through
//! `/usr/bin/sandbox-exec` ([`SeatbeltLauncher`]), and the one host-wide watcher that turns the
//! unified log's Sandbox lines into [`Denial`](crate::Denial)s.

#[cfg(target_os = "macos")]
mod launcher;
mod profile;
#[cfg(target_os = "macos")]
mod watch;

#[cfg(target_os = "macos")]
pub use launcher::SeatbeltLauncher;
pub use profile::SeatbeltProfile;
