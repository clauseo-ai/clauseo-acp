//! The sandbox's set-up half in the app (R17 PR-D3a, decision record 0051): the host gets the
//! sandbox's detection and, on Windows, tier B's one-time set-up, with no launchers yet (D3b
//! adds Windows' launcher and Seatbelt). Under the host's rule a seam without launchers never
//! drives launch gating: launches stay as in a build without the sandbox (`not_built`), and the
//! protection view's `set_up_state` carries the sandbox's own state for the set-up screen.
//!
//! - Windows: `UserSetup` on the data folder. Its `run()` (through the `SandboxSetUp` pipe,
//!   blocking for minutes) runs only from the app's set-up action, `RetryProtection`, on the
//!   host's set-up thread; never from the installer, the UI thread or the host's actor.
//! - macOS and Linux: detection only, no set-up. Linux's detection says no jail on this OS
//!   (`unsupported { why: os }`); macOS' says tier A, which the host keeps out of the view
//!   (no set-up, so no `set_up_state`; the computer `not_built`) until D3b's Seatbelt.
//! - The first detection runs in `Host::start`, on the host's start thread, never the UI's,
//!   bounded at 5 s by the host (a later answer is a failed probe), so quit waits no longer.
//! - The `sandbox` feature (on by default) gates this; without it the host has no seam, as
//!   before.

use std::path::Path;
#[cfg(feature = "sandbox")]
use std::sync::Arc;

use clauseo_host::HostConfig;
#[cfg(feature = "sandbox")]
use clauseo_sandbox::SandboxSetup;

/// `config` with Windows' seam: tier B's set-up for the data folder `data_dir`, and its
/// detection.
#[cfg(all(feature = "sandbox", windows))]
pub(crate) fn with_sandbox(config: HostConfig, data_dir: &Path) -> HostConfig {
    let setup: Arc<dyn SandboxSetup> =
        Arc::new(clauseo_sandbox::UserSetup::new(data_dir.to_path_buf()));
    config.with_sandbox(seam(Some(setup)))
}

/// `config` with this OS's detection and no set-up (macOS, Linux).
#[cfg(all(feature = "sandbox", not(windows)))]
pub(crate) fn with_sandbox(config: HostConfig, _data_dir: &Path) -> HostConfig {
    config.with_sandbox(seam(None))
}

/// `config` as it was: a build without the `sandbox` feature gives the host no seam.
#[cfg(not(feature = "sandbox"))]
pub(crate) fn with_sandbox(config: HostConfig, _data_dir: &Path) -> HostConfig {
    config
}

/// The seam: this OS's detection over `setup`, and no launchers (R17 PR-D3a).
#[cfg(feature = "sandbox")]
fn seam(setup: Option<Arc<dyn SandboxSetup>>) -> clauseo_host::Sandbox {
    let detect_with = setup.clone();
    let detect: clauseo_host::Detect =
        Arc::new(move || clauseo_sandbox::detect(detect_with.as_deref()));
    clauseo_host::Sandbox::new(detect, setup)
}

#[cfg(all(test, feature = "sandbox"))]
mod tests {
    /// D3a's seam never has launchers, so it never gates launches.
    #[test]
    fn the_seam_has_no_launchers() {
        assert!(super::seam(None).launchers.is_none());
    }

    /// Linux has no jail: its detection says so, with nothing to set up.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_detects_no_jail_on_this_os() {
        let found = (super::seam(None).detect)();
        assert_eq!(
            found.reason,
            Some(clauseo_sandbox::NoSandbox::UnsupportedOs)
        );
    }
}
