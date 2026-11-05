//! The envelope values that are the same for every day and tally of one run of the app.

use crate::schema::{Arch, Channel, InstallType, Language, Platform, Version};

/// What the app says about itself in each envelope. Desktop fills it in at start (research 37
/// §2.1; the brief's "Envelope values") and again when the language changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct About {
    /// Desktop's `CARGO_PKG_VERSION`.
    pub version: Version,
    /// A build constant.
    pub channel: Channel,
    /// The OS and its major version.
    pub platform: Platform,
    /// The CPU architecture.
    pub arch: Arch,
    /// `ui.language`. Day reports only.
    pub language: Language,
    /// Per user or per machine. Tallies only.
    pub install_type: InstallType,
    /// Whether any policy source was read. Tallies only.
    pub managed: bool,
}
