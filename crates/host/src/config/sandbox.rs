//! The sandbox seam (decision record 0051, R17 PR-B1): how the host learns this computer's
//! protection and runs the one-time set-up. Without it, the computer is `Unsupported { why:
//! not_built }` and the two protection commands are `command.invalid`; the host never calls
//! `clauseo_sandbox::detect` on its own, since on macOS that would report tier A while nothing
//! jails the agent.

use std::fmt;
use std::sync::Arc;

use clauseo_sandbox::{Detection, SandboxSetup};

/// The detection the host calls, off its actor: at start, and after each set-up, removal or
/// check. Desktop passes `move || clauseo_sandbox::detect(setup.as_deref())`.
pub type Detect = Arc<dyn Fn() -> Detection + Send + Sync>;

/// What the host holds of the sandbox: its detection and, where the OS has one (Windows' tier
/// B), the set-up. Built with [`Sandbox::new`]; later steps add fields (R17 PR-B2's launchers),
/// so it is non-exhaustive.
#[derive(Clone)]
#[non_exhaustive]
pub struct Sandbox {
    /// This computer's tier, and why it is C.
    pub detect: Detect,
    /// The one-time set-up, where the OS has one; `None` on macOS and Linux.
    pub setup: Option<Arc<dyn SandboxSetup>>,
    /// The set-up thread fails to start, as the OS can refuse a thread (tests only; set it with
    /// [`Sandbox::failing_spawn`]).
    #[cfg(feature = "test-support")]
    pub fail_spawn: bool,
}

impl Sandbox {
    /// The seam from its detection and set-up.
    #[must_use]
    pub fn new(detect: Detect, setup: Option<Arc<dyn SandboxSetup>>) -> Self {
        Self {
            detect,
            setup,
            #[cfg(feature = "test-support")]
            fail_spawn: false,
        }
    }

    /// The set-up thread fails to start, as the OS can refuse a thread, for tests of the answer.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn failing_spawn(mut self) -> Self {
        self.fail_spawn = true;
        self
    }
}

impl fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandbox")
            .field("setup", &self.setup.is_some())
            .finish_non_exhaustive()
    }
}
